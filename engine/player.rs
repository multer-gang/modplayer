use std::{
    array,
    f32::consts::PI,
    io::{stdout, Write}, sync::LazyLock,
};

use crate::engine::{lut, module::{Effect, PlaybackMode}};

use super::module::{Column, LoopType, Module, Note, Row, VolEffect};
use fixed::{traits::ToFixed, types::{I32F32, U0F32, U32F32}};
use sdl2::audio::AudioCallback;

#[derive(Default, Debug, Clone, Copy, clap::ValueEnum)]
pub enum Interpolation {
    #[default]
    None,
    Linear,
    Sinc16,
    Sinc32,
    Sinc64,
    Sinc64Fast
}

#[derive(Clone)]
struct Channel<'a> {
    module: &'a Module,

    current_sample_index: u8,
    playing: bool,
    freq: U32F32,
    base_freq: U32F32, // For going back from arpeggio
    current_note: u8,
    position: U32F32,
    backwards: bool,

    porta_memory: u8,     // Exx, Fxx, Gxx
    last_note: u8,        // Gxx
    offset_memory: u8,    // Oxx
    volume_memory: u8,    // Dxy
    global_volume_memory: u8, // Wxy
    retrigger_memory: u8, // Qxy
    retrigger_ticks: u8,  // Qxy
    arpeggio_memory: u8,  // Jxy
    arpeggio_selector: u8,
    arpeggio_state: bool,
    s3m_effect_memory: u8, // S3M only

    volume: u8,
    // panning: i8,
}

const PERIOD: u32 = 14317056;

fn period(freq: u32) -> u32 {
    PERIOD / freq
}

fn freq_from_period(period: u32) -> U32F32 {
    U32F32::from_num(PERIOD) / U32F32::from_num(period)
}

impl Channel<'_> {
    fn porta_up(&mut self, linear: bool, ticks_passed: u8, mut value: u8) {
        if value != 0 {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    self.s3m_effect_memory = value,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    self.porta_memory = value,
                _ => todo!(),
            }
        } else {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    value = self.s3m_effect_memory,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    value = self.porta_memory,
                _ => todo!(),
            }
        }

        if linear {
            match value & 0xF0 {
                // Detect fine-iness
                0xE0 => { // Extra fine
                    if ticks_passed == 0 {
                        self.freq *= U32F32::from(lut::FINE_LINEAR_UP[(value & 0xF) as usize]);
                    }
                }
                0xF0 => { // Fine
                    if ticks_passed == 0 {
                        self.freq *= U32F32::from(lut::LINEAR_UP[(value & 0xF) as usize]);
                    }
                }
                _ => { // Regular
                    if ticks_passed > 0 {
                        self.freq *= U32F32::from(lut::LINEAR_UP[value as usize]);
                    }
                }
            }
        } else {
            // Amiga slide
            match value & 0xF0 {
                0xE0 => {
                    if ticks_passed == 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) - ((value & 0xF) as u32))
                    }
                }
                0xF0 => {
                    if ticks_passed == 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) - ((value & 0xF) as u32 * 4))
                    }
                }
                _ => {
                    if ticks_passed > 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) - (value as u32 * 4))
                    }
                },
            }
        }
    }

    fn porta_down(&mut self, linear: bool, ticks_passed: u8, mut value: u8) {
        if value != 0 {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    self.s3m_effect_memory = value,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    self.porta_memory = value,
                _ => todo!(),
            }
        } else {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    value = self.s3m_effect_memory,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    value = self.porta_memory,
                _ => todo!(),
            }
        }

        if linear {
            match value & 0xF0 {
                // Detect fine-iness
                0xE0 => { // Extra fine
                    if ticks_passed == 0 {
                        self.freq *= U32F32::from(lut::FINE_LINEAR_DOWN[(value & 0xF) as usize]);
                    }
                }
                0xF0 => { // Fine
                    if ticks_passed == 0 {
                        self.freq *= U32F32::from(lut::LINEAR_DOWN[(value & 0xF) as usize]);
                    }
                }
                _ => { // Regular
                    if ticks_passed > 0 {
                        self.freq *= U32F32::from(lut::LINEAR_DOWN[value as usize]);
                    }
                }
            }
        } else {
            // Amiga slide
            match value & 0xF0 {
                0xE0 => {
                    if ticks_passed == 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) + ((value & 0xF) as u32))
                    }
                }
                0xF0 => {
                    if ticks_passed == 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) + ((value & 0xF) as u32 * 4))
                    }
                }
                _ => {
                    if ticks_passed > 0 {
                        self.freq = freq_from_period(period(self.freq.to_num::<u32>()) + (value as u32 * 4))
                    }
                },
            }
        }
    }

    fn tone_portamento(&mut self, note: Note, linear: bool, mut value: u8) {
        if value != 0 {
            self.porta_memory = value;
        } else {
            value = self.porta_memory;
        }

        match note {
            Note::On(key) => self.last_note = key,
            _ => {}
        }

        let desired_freq = lut::PITCH_TABLE[self.last_note as usize]
            * U32F32::from(self.module.samples[self.current_sample_index as usize].base_frequency);

        if linear {
            if self.freq < desired_freq {
                self.freq = self.freq * lut::LINEAR_UP[value as usize];
                if self.freq > desired_freq {
                    self.freq = desired_freq
                }
            } else if self.freq > desired_freq {
                self.freq = self.freq * lut::LINEAR_DOWN[value as usize].to_fixed::<U32F32>();
                if self.freq < desired_freq {
                    self.freq = desired_freq
                }
            }
        } else {
            // Amiga slides
            let desired_period: u32 = period(desired_freq.to_num::<u32>());
            let mut tmp_period: u32;

            if self.freq < desired_freq {
                tmp_period = period(self.freq.to_num::<u32>()).saturating_sub(value as u32 * 4);
                if tmp_period < desired_period {
                    tmp_period = desired_period
                }
            } else if self.freq > desired_freq {
                tmp_period = period(self.freq.to_num::<u32>()) + (value as u32 * 4);
                if tmp_period > desired_period {
                    tmp_period = desired_period
                }
            } else {
                tmp_period = desired_period;
            }
            self.freq = freq_from_period(tmp_period);
        }
    }

    fn vol_slide(&mut self, mut value: u8, ticks_passed: u8) {
        if value != 0 {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    self.s3m_effect_memory = value,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    self.volume_memory = value,
                _ => todo!(),
            }
        } else {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    value = self.s3m_effect_memory,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    value = self.volume_memory,
                _ => todo!(),
            }
        }

        let upper = (value & 0xF0) >> 4;
        let lower = value & 0x0F;

        if lower == 0xF && upper > 0 {
            // fine up
            if ticks_passed == 0 {
                self.volume += upper
            }
        } else if upper == 0xF && lower > 0 {
            // fine down
            if ticks_passed == 0 {
                self.volume = self.volume.saturating_sub(lower)
            }
        } else if lower == 0 {
            if ticks_passed > 0 || self.module.fast_volume_slides {
                self.volume += upper
            }
        } else {
            if ticks_passed > 0 || self.module.fast_volume_slides {
                self.volume = self.volume.saturating_sub(lower)
            }
        }

        if self.volume > 64 {
            self.volume = 64
        };
    }

    fn retrigger(&mut self, mut value: u8) {
        if value != 0 {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    self.s3m_effect_memory = value,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    self.retrigger_memory = value,
                _ => todo!(),
            }
        } else {
            match self.module.mode {
                super::module::PlaybackMode::S3M(_) =>
                    value = self.s3m_effect_memory,
                super::module::PlaybackMode::IT | super::module::PlaybackMode::ITSample =>
                    value = self.retrigger_memory,
                _ => todo!(),
            }
        }

        match (value & 0xF0) >> 4 {
            // Volume change
            // TODO last used value for XM
            //0 => {}
            1 => self.volume -= 1,
            2 => self.volume -= 2,
            3 => self.volume -= 4,
            4 => self.volume -= 8,
            5 => self.volume -= 16,
            6 => self.volume = (self.volume * 3) / 2,
            7 => self.volume /= 2,

            9 => self.volume += 1,
            0xA => self.volume += 2,
            0xB => self.volume += 4,
            0xC => self.volume += 8,
            0xD => self.volume += 16,
            0xE => self.volume = (self.volume * 2) / 3,
            0xF => self.volume *= 2,

            _ => {}
        }

        if self.retrigger_ticks >= value & 0x0F {
            self.position = U32F32::const_from_int(0);
            self.retrigger_ticks = 0;
        };

        self.retrigger_ticks += 1;

        if self.volume > 64 {
            self.volume = 64
        };
    }

    fn arpeggio(&mut self, mut value: u8) {
        if value != 0 {
            self.arpeggio_memory = value;
        } else {
            value = self.arpeggio_memory;
        }

        match self.arpeggio_selector {
            0 => self.freq = self.base_freq,
            1 => self.freq = lut::PITCH_TABLE[self.current_note as usize + ((value as usize & 0xF0) >> 4)]
                * U32F32::from(self.module.samples[self.current_sample_index as usize]
                    .base_frequency),
            2 => self.freq = lut::PITCH_TABLE[self.current_note as usize + (value as usize & 0x0F)]
                * U32F32::from(self.module.samples[self.current_sample_index as usize]
                    .base_frequency),
            _ => {}
        }

        self.arpeggio_selector = (self.arpeggio_selector + 1) % 3;
        self.arpeggio_state = true;
    }

    fn process(&mut self, samplerate: u32, interpolation: Interpolation) -> i16 {
        if self.current_sample_index as usize >= self.module.samples.len() { return 0 }

        let sample = &self.module.samples[self.current_sample_index as usize];
        if !self.playing || sample.audio.len() == 0 {
            return 0;
        };

        if self.backwards {
            if self.position.to_num::<u32>() <= sample.loop_start {
                self.backwards = false
            } else {
                self.position -= U32F32::from(self.freq) / U32F32::from(samplerate);
            }
        } else {
            self.position += U32F32::from(self.freq) / U32F32::from(samplerate);
        }

        if sample.loop_end > 0 {
            if self.position.to_num::<u32>() > sample.loop_end - 1 {
                match sample.loop_type {
                    // LoopType::Forward => self.position = U32F32::from(sample.loop_start),
                    LoopType::Forward => self.position -= U32F32::from(sample.loop_end - sample.loop_start),
                    LoopType::PingPong => {
                        self.backwards = true;
                        self.position -= U32F32::from(self.freq) / U32F32::from(samplerate);
                    } // self.position -= 1.0 or 2.0 does not work as the program errors with out of bounds
                    _ => {}
                }
            }
        }

        // Prevent out of bounds, but it doesn't seem to be working reliably
        if self.position.to_num::<usize>() >= sample.audio.len() - 1
            && matches!(sample.loop_type, LoopType::None)
        {
            self.playing = false;
            self.backwards = false;
        }

        if !self.playing {
            return 0;
        };

        match interpolation {
            _ => {
                (I32F32::from(sample.audio[self.position.to_num::<usize>()])
                    * (I32F32::from(self.volume) / I32F32::const_from_int(64))
                    * (I32F32::from(sample.global_volume) / I32F32::const_from_int(64))
                )
                    .to_num::<i16>()
            }
        }
    }
}

pub struct Player<'a> {
    pub module: &'a Module,

    pub samplerate: u32,
    pub interpolation: Interpolation,
    pub global_volume: u8,

    pub current_position: u8,
    pub current_pattern: u8,
    current_row: u16,

    current_tempo: u8,
    current_speed: u8,

    tick_counter: u32,
    ticks_passed: u8,

    channels: [Channel<'a>; 64],
}

impl Player<'_> {
    pub fn from_module(module: &Module, samplerate: u32) -> Player<'_> {
        Player {
            module,

            samplerate,
            interpolation: Interpolation::Linear,
            global_volume: module.initial_global_volume,

            current_position: 0,
            current_pattern: module.playlist[0],
            current_row: 65535,

            current_tempo: module.initial_tempo,
            current_speed: module.initial_speed,

            tick_counter: 0,
            ticks_passed: 0,

            channels: array::from_fn(|_| Channel {
                module: module,

                current_sample_index: 0,
                playing: false,
                freq: U32F32::const_from_int(8363),
                base_freq: U32F32::const_from_int(8363),
                current_note: 0,
                position: U32F32::const_from_int(0),
                backwards: false,

                porta_memory: 0,
                last_note: 0,
                offset_memory: 0,
                volume_memory: 0,
                global_volume_memory: 0,
                retrigger_memory: 0,
                retrigger_ticks: 0,
                arpeggio_memory: 0,
                arpeggio_selector: 0,
                arpeggio_state: false,
                s3m_effect_memory: 0,

                volume: 64,
                // panning: 0
            }),
        }
    }

    pub fn process(&mut self) -> i32 {
        let mut out = 0i32;

        for c in self.channels.iter_mut() {
            if c.playing {
                let mut tmp = c.process(self.samplerate, self.interpolation) as i32
                    * self.module.mixing_volume as i32
                    * self.global_volume as i32
                    * 2;

                if !matches!(self.module.mode, PlaybackMode::IT | PlaybackMode::ITSample) {
                    tmp *= 2;
                }

                out = out.saturating_add(tmp as i32);
            }
        }

        if self.tick_counter >= ((self.samplerate as f32 * 2.5) / self.current_tempo as f32) as u32
        {
            self.ticks_passed += 1;
            self.tick_counter = 0;
            if self.ticks_passed >= self.current_speed {
                self.advance_row();
                self.play_row();
            }
            self.process_tick();
        } else {
            self.tick_counter += 1;
        }

        out
    }

    fn global_vol_slide(&mut self, value: u8) {
        let upper = (value & 0xF0) >> 4;
        let lower = value & 0x0F;

        if lower == 0xF && upper > 0 {
            // fine up
            if self.ticks_passed == 0 {
                self.global_volume = self.global_volume.saturating_add(upper)
            }
        } else if upper == 0xF && lower > 0 {
            // fine down
            if self.ticks_passed == 0 {
                self.global_volume = self.global_volume.saturating_sub(lower)
            }
        } else if lower == 0 {
            if self.ticks_passed > 0 || self.module.fast_volume_slides {
                self.global_volume = self.global_volume.saturating_add(upper)
            }
        } else {
            if self.ticks_passed > 0 || self.module.fast_volume_slides {
                self.global_volume = self.global_volume.saturating_sub(lower)
            }
        }

        if self.global_volume > max_global_volume(&self.module.mode) {
            self.global_volume = max_global_volume(&self.module.mode)
        };
    }

    fn process_tick(&mut self) {
        if self.current_row == 65535 {
            return;
        };
        let row = &self.module.patterns[self.current_pattern as usize][self.current_row as usize];

        for (i, col) in row.iter().enumerate() {
            let channel = &mut self.channels[i];

            match col.effect {
                Effect::PortaUp(value) => {
                    channel.porta_up(self.module.linear_freq_slides, self.ticks_passed, value);
                }
                Effect::PortaDown(value) => {
                    channel.porta_down(self.module.linear_freq_slides, self.ticks_passed, value);
                }
                Effect::TonePorta(value) => {
                    if self.ticks_passed <= 0 {return};
                    channel.tone_portamento(col.note, self.module.linear_freq_slides, value)
                }
                Effect::VolSlideTonePorta(value) => {
                    channel.vol_slide(value, self.ticks_passed);
                    if self.ticks_passed <= 0 {return};
                    channel.tone_portamento(col.note, self.module.linear_freq_slides, 0);
                }
                Effect::VolSlideVibrato(value) => {
                    channel.vol_slide(value, self.ticks_passed);
                },
                Effect::VolSlide(value) => channel.vol_slide(value, self.ticks_passed),
                Effect::Retrig(value) => channel.retrigger(value),
                Effect::Arpeggio(value) => channel.arpeggio(value),
                Effect::Vibrato(value) => {
                    if value != 0 && matches!(self.module.mode, super::module::PlaybackMode::S3M(_)) {
                        channel.s3m_effect_memory = value;
                    }
                },
                Effect::GlobalVolSlide(mut value) => {
                    if value != 0 {
                        channel.global_volume_memory = value
                    } else {
                        value = channel.global_volume_memory
                    }

                    self.global_vol_slide(value);
                },
                Effect::None(value) => {
                    if value != 0 && matches!(self.module.mode, super::module::PlaybackMode::S3M(_)) {
                        channel.s3m_effect_memory = value;
                    }
                }
                _ => {}
            }
        }

        // print!(
        //     "[Position {}, Pattern {}, Row {}]\x1b[K\n\x1b[K\nChannels:\x1b[K\n",
        //     self.current_position, self.current_pattern, self.current_row
        // );

        // for (i, channel) in self.channels.iter().enumerate() {
        //     if !channel.playing {
        //         println!(" {:>3} -\x1b[K", i + 1);
        //     } else {
        //         println!(
        //             " {:>3} : sample {:<4}  volume {:>05.2}   freq {:<6.2}\x1b[K",
        //             i + 1,
        //             channel.current_sample_index,
        //             channel.volume,
        //             channel.freq
        //         );
        //     }
        // }

        // print!("\x1b[{}F", self.channels.len() + 3);

        // stdout().flush().unwrap();
    }

    fn advance_row(&mut self) {
        if self.current_row == 65535 {
            self.current_row = 0;
            self.ticks_passed = 0;
            return;
        };

        let row = &self.module.patterns[self.current_pattern as usize][self.current_row as usize];
        let mut pos_jump_enabled = false;
        let mut pos_jump_to = 0u8;

        let mut pat_break_enabled = false;
        let mut pat_break_to = 0u8;

        for col in row.iter() {
            match col.effect {
                Effect::PosJump(position) => {
                    pos_jump_enabled = true;
                    pos_jump_to = position
                }
                Effect::PatBreak(row) => {
                    pat_break_enabled = true;
                    pat_break_to = match self.module.mode {
                        super::module::PlaybackMode::MOD | super::module::PlaybackMode::S3M(_) =>
                            (row & 0xF) + ((row >> 4) * 10),
                        _ => row,
                    }
                }
                _ => {}
            }
        }

        self.ticks_passed = 0;
        if self.current_row == self.module.patterns[self.current_pattern as usize].len() as u16 {
            self.current_row = 0;
        } else {
            self.current_row += 1;
            if pos_jump_enabled {
                self.current_row = 0;
                self.current_position = pos_jump_to;
                self.current_pattern = self.module.playlist[self.current_position as usize];
            }

            if pat_break_enabled {
                self.current_row = pat_break_to as u16;
                self.current_position += 1;
                self.current_pattern = self.module.playlist[self.current_position as usize];

                if self.current_pattern == 255 {
                    self.current_position = 0;
                    self.current_pattern = self.module.playlist[self.current_position as usize];
                }
            }
        }

        loop {
            if self.current_pattern == 254 {
                self.current_position += 1;
                self.current_pattern = self.module.playlist[self.current_position as usize];
            } else {
                break;
            }
        }
        if self.current_row as usize == self.module.patterns[self.current_pattern as usize].len() {
            self.current_row = 0;
            self.current_position += 1;
            self.current_pattern = self.module.playlist[self.current_position as usize];

            loop {
                if self.current_pattern == 254 {
                    self.current_position += 1;
                    self.current_pattern = self.module.playlist[self.current_position as usize];
                } else {
                    break;
                }
            }

            if self.current_pattern == 255 {
                // End of song marker
                std::process::exit(0);
            }
        };
    }

    fn play_row(&mut self) {
        let row = &self.module.patterns[self.current_pattern as usize][self.current_row as usize];
        // let mut row_string = String::new();
        // row_string.push_str(&format!("{:0>2} | ", self.current_row));
        // for (i, col) in row.iter().enumerate() {
        //     row_string.push_str(&format!("{} \x1b[0m| ", &format_col(col)));
        //     // if i >= 16 {
        //     //     break;
        //     // }
        // }
        // println!("{}", row_string);

        print!(
            "Position {}, Pattern {}, Row {}\x1b[K\r",
            self.current_position, self.current_pattern, self.current_row
        );
        stdout().flush().unwrap();

        for (i, col) in row.iter().enumerate() {
            let channel = &mut self.channels[i];

            /* match col.effect {
                _ => {}
                //TODO effects
            } */

            match col.vol {
                // TODO volume commands
                VolEffect::None => {}
                VolEffect::FineVolSlideUp(_) => {}
                VolEffect::FineVolSlideDown(_) => {}
                VolEffect::VolSlideUp(_) => {}
                VolEffect::VolSlideDown(_) => {}
                VolEffect::PortaDown(_) => {}
                VolEffect::PortaUp(_) => {}
                VolEffect::TonePorta(_) => {}
                VolEffect::VibratoDepth(_) => {}
                VolEffect::SetPan(_) => {}
                VolEffect::Volume(volume) => channel.volume = volume,
            }

            match col.effect {
                Effect::SetSpeed(speed) => self.current_speed = speed,
                Effect::SetTempo(tempo) => self.current_tempo = tempo,
                Effect::Arpeggio(_) => channel.arpeggio_selector = 0,
                Effect::SetGlobalVol(vol) => if vol <= max_global_volume(&self.module.mode) {self.global_volume = vol},
                _ => {}
            }

            if channel.arpeggio_state {
                if !matches!(self.module.mode, PlaybackMode::S3M(_)) ||
                    !matches!(col.effect, Effect::PortaUp(_) | Effect::PortaDown(_))
                {
                    channel.freq = channel.base_freq;
                }
                channel.arpeggio_state = false;
            }

            if col.instrument != 0 {
                channel.current_sample_index = col.instrument - 1;

                if matches!(col.vol, VolEffect::None) && (channel.current_sample_index as usize) < self.module.samples.len() {
                    channel.volume = self.module.samples[channel.current_sample_index as usize]
                        .default_volume
                }
            }

            match col.note {
                Note::None => {}
                Note::On(note) => {
                    if !matches!(col.effect, Effect::TonePorta(_))
                        && !matches!(col.vol, VolEffect::TonePorta(_))
                    {
                        channel.playing = true;
                        channel.position = match col.effect {
                            Effect::SampleOffset(position) => {
                                if position != 0 {
                                    channel.offset_memory = position
                                };
                                U32F32::from(channel.offset_memory as u32 * 256)
                            }
                            _ => U32F32::const_from_int(0),
                        };
                        if channel.current_sample_index as usize >= self.module.samples.len() {
                            channel.playing = false;
                        } else {
                            channel.current_note = note;
                            channel.base_freq = lut::PITCH_TABLE[note as usize]
                                * U32F32::from(self.module.samples[channel.current_sample_index as usize]
                                    .base_frequency);
                            channel.freq = channel.base_freq;
                        }
                    }
                }
                Note::Fade => {}
                Note::Cut => channel.playing = false,
                Note::Off => channel.playing = false,
            }
        }
    }
}

impl AudioCallback for Player<'_> {
    type Channel = i32;

    fn callback(&mut self, out: &mut [i32]) {
        for s in out.iter_mut() {
            *s = self.process();
        }
    }
}

fn max_global_volume(mode: &PlaybackMode) -> u8 {
    match mode {
        PlaybackMode::S3M(_) => 64,
        PlaybackMode::IT => 128,
        PlaybackMode::ITSample => 128,
        _ => 64,
    }
}

fn format_note(note: Note) -> String {
    let real_note: u8;
    match note {
        Note::None => return "...".to_owned(),
        Note::On(n) => real_note = n,
        Note::Fade => return "~~~".to_owned(),
        Note::Cut => return "^^^".to_owned(),
        Note::Off => return "===".to_owned(),
    }

    let mut out = String::new();

    out.push_str(match real_note % 12 {
        0 => "C-",
        1 => "C#",
        2 => "D-",
        3 => "D#",
        4 => "E-",
        5 => "F-",
        6 => "F#",
        7 => "G-",
        8 => "G#",
        9 => "A-",
        10 => "A#",
        11 => "B-",
        _ => unreachable!()
    });

    out.push_str(format!("{}", real_note/12).as_str());

    out
}

fn format_col(col: &Column) -> String {
    let instrument = if col.instrument == 0 { "\x1b[37m..".to_string() } else { format!("\x1b[96m{:0>2}", col.instrument) };
    let volume = format_vol(&col.vol);
    // let fx = if col.effect == 0 { ".".to_string() } else { format!("{}", (0x40+col.effect) as char) };
    // let fxvalue = if col.effect_value == 0 { if col.effect != 0 { "00".to_string() } else { "..".to_string() } } else { format!("{:0>2X}", col.effect_value) };
    let fx = format_effect(&col.effect);

    // format!("{} {instrument} {volume} {fx}{fxvalue}", format_note(col.note))
    format!("\x1b[0m\x1b[97m{} {instrument} {volume} {fx}", format_note(col.note))
}

fn format_vol(vol: &VolEffect) -> String {
    match vol {
        VolEffect::FineVolSlideUp(value) => format!("a{:0>2}", value).to_owned(),
        VolEffect::FineVolSlideDown(value) => format!("b{:0>2}", value).to_owned(),
        VolEffect::VolSlideUp(value) => format!("c{:0>2}", value).to_owned(),
        VolEffect::VolSlideDown(value) => format!("d{:0>2}", value).to_owned(),
        VolEffect::PortaDown(value) => format!("e{:0>2}", value).to_owned(),
        VolEffect::PortaUp(value) => format!("f{:0>2}", value).to_owned(),
        VolEffect::TonePorta(value) => format!("g{:0>2}", value).to_owned(),
        VolEffect::VibratoDepth(value) => format!("h{:0>2}", value).to_owned(),
        VolEffect::SetPan(value) => format!("p{:0>2}", value).to_owned(),
        VolEffect::Volume(value) => format!("\x1b[92mv{:0>2}", value).to_owned(),
        VolEffect::None => " \x1b[37m..".to_owned(),
    }
}

fn format_effect(effect: &Effect) -> String {
    match effect {
        Effect::None(value) => if *value != 0u8 { format!("\x1b[37m.{:0>2X}", value) } else { "\x1b[37m...".to_owned() },

        Effect::SetSpeed(value) => format!("\x1b[91mA{:0>2X}", value),           // Axx
        Effect::PosJump(value) => format!("\x1b[91mB{:0>2X}", value),            // Bxx
        Effect::PatBreak(value) => format!("\x1b[91mC{:0>2X}", value),           // Cxx
        Effect::VolSlide(value) => format!("\x1b[92mD{:0>2X}", value),           // Dxy
        Effect::PortaDown(value) => format!("\x1b[93mE{:0>2X}", value),          // Exx
        Effect::PortaUp(value) => format!("\x1b[93mF{:0>2X}", value),            // Fxx
        Effect::TonePorta(value) => format!("\x1b[93mG{:0>2X}", value),          // Gxx
        Effect::Vibrato(value) => format!("\x1b[93mH{:0>2X}", value),            // Hxy
        Effect::Tremor(value) => format!("\x1b[97mI{:0>2X}", value),             // Ixy
        Effect::Arpeggio(value) => format!("\x1b[97mJ{:0>2X}", value),           // Jxy
        Effect::VolSlideVibrato(value) => format!("\x1b[92mK{:0>2X}", value),    // Kxy
        Effect::VolSlideTonePorta(value) => format!("\x1b[92mL{:0>2X}", value),  // Lxy
        Effect::SetChanVol(value) => format!("\x1b[92mM{:0>2X}", value),         // Mxx
        Effect::ChanVolSlide(value) => format!("\x1b[92mN{:0>2X}", value),       // Nxy
        Effect::SampleOffset(value) => format!("\x1b[97mO{:0>2X}", value),       // Oxx
        Effect::PanSlide(value) => format!("\x1b[96mP{:0>2X}", value),           // Pxy
        Effect::Retrig(value) => format!("\x1b[97mQ{:0>2X}", value),             // Qxy
        Effect::Tremolo(value) => format!("\x1b[92mR{:0>2X}", value),            // Rxy

        Effect::GlissandoControl(bool) => if *bool { "\x1b[97mS11".to_owned() } else { "\x1b[97mS10".to_owned() },    // S1x
        Effect::SetFinetune(value) => format!("\x1b[97mS2{:0>1X}", value),           // S2x
        Effect::SetVibratoWaveform(value) => format!("\x1b[97mS3{:0>1X}", value),    // S3x
        Effect::SetTremoloWaveform(value) => format!("\x1b[97mS4{:0>1X}", value),    // S4x
        Effect::SetPanbrelloWaveform(value) => format!("\x1b[97mS5{:0>1X}", value),  // S5x
        Effect::FinePatternDelay(value) => format!("\x1b[97mS6{:0>1X}", value),      // S6x

        Effect::PastNoteCut => "\x1b[97mS70".to_owned(),      // S70
        Effect::PastNoteOff => "\x1b[97mS71".to_owned(),      // S71
        Effect::PastNoteFade => "\x1b[97mS72".to_owned(),     // S72
        Effect::NNANoteCut => "\x1b[97mS73".to_owned(),       // S73
        Effect::NNANoteContinue => "\x1b[97mS74".to_owned(),  // S74
        Effect::NNANoteOff => "\x1b[97mS75".to_owned(),       // S75
        Effect::NNANoteFade => "\x1b[97mS76".to_owned(),      // S76
        Effect::VolEnvOff => "\x1b[97mS77".to_owned(),        // S77
        Effect::VolEnvOn => "\x1b[97mS78".to_owned(),         // S78
        Effect::PanEnvOff => "\x1b[97mS79".to_owned(),        // S79
        Effect::PanEnvOn => "\x1b[97mS7A".to_owned(),         // S7A
        Effect::PitchEnvOff => "\x1b[97mS7B".to_owned(),      // S7B
        Effect::PitchEnvOn => "\x1b[97mS7C".to_owned(),       // S7C

        Effect::SetPan(value) => format!("\x1b[97mS8{:0>1X}", value),          // S8x
        Effect::SoundControl(value) => format!("\x1b[97mS9{:0>1X}", value),    // S9x
        Effect::HighOffset(value) => format!("\x1b[97mSA{:0>1X}", value),      // SAx
        Effect::PatLoopStart => "SB0".to_owned(),        // SB0
        Effect::PatLoop(value) => format!("\x1b[97mSB{:0>1X}", value),         // SBx
        Effect::NoteCut(value) => format!("\x1b[97mSC{:0>1X}", value),         // SCx
        Effect::NoteDelay(value) => format!("\x1b[97mSD{:0>1X}", value),       // SDx
        Effect::PatDelay(value) => format!("\x1b[97mSE{:0>1X}", value),        // SEx
        Effect::SetActiveMacro(value) => format!("\x1b[97mSF{:0>1X}", value),  // SFx

        Effect::DecTempo(value) => format!("\x1b[91mT{:0>2X}", value),        // T0x
        Effect::IncTempo(value) => format!("\x1b[91mT{:0>2X}", value),        // T1x
        Effect::SetTempo(value) => format!("\x1b[91mT{:0>2X}", value),        // Txx
        Effect::FineVibrato(value) => format!("\x1b[93mU{:0>2X}", value),     // Uxy
        Effect::SetGlobalVol(value) => format!("\x1b[91mV{:0>2X}", value),    // Vxx
        Effect::GlobalVolSlide(value) => format!("\x1b[91mW{:0>2X}", value),  // Wxy
        Effect::FineSetPan(value) => format!("\x1b[96mX{:0>2X}", value),      // Xxx
        Effect::Panbrello(value) => format!("\x1b[96mY{:0>2X}", value),       // Yxy
        Effect::MIDIMacro(value) => format!("\x1b[97mZ{:0>2X}", value),       // Zxx
    }
}