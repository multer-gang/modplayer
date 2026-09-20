use crate::format_mod::MODTracker::{Fasttracker, Noisetracker, Protracker, Soundtracker};

use super::module::{
    Column, Effect, LoopType, MODOptions, Module, ModuleInterface, Note, Pattern, PlaybackMode,
    Row, Sample, VolEffect,
};
use anyhow::{bail, Result};
use byteorder::{BigEndian, LittleEndian, ReadBytesExt};
use std::{
    io::{self, SeekFrom},
    ops::Index,
};

const MOD_PERIOD_TABLE: [u16; 60] = [
    1712, 1616, 1524, 1440, 1356, 1280, 1208, 1140, 1076, 1016, 960, 906, 856, 808, 762, 720, 678,
    640, 604, 570, 538, 508, 480, 453, 428, 404, 381, 360, 339, 320, 302, 285, 269, 254, 240, 226,
    214, 202, 190, 180, 170, 160, 151, 143, 135, 127, 120, 113, 107, 101, 95, 90, 85, 80, 75, 71,
    67, 63, 60, 56,
];

fn find_note_value(period: u16) -> Option<u8> {
    MOD_PERIOD_TABLE
        .iter()
        .position(|&p| p <= period + 2)
        .map(|index| index as u8)
}

#[derive(Debug)]
enum MODTracker {
    Soundtracker,
    Noisetracker,
    Protracker,
    Fasttracker,
    ScreamTracker,
    ProtrackerCompatible,
    GenericMultichannel,
}

#[derive(Debug, Default)]
pub struct MODSample {
    sample_name: [u8; 22],
    length: u32,
    finetune: i8,
    volume: u8,
    loop_start: u32,
    loop_length: u32,

    // Public
    pub audio: Vec<i16>,
}

#[derive(Debug)]
pub struct MODModule {
    // FILE STRUCTURE
    song_name: [u8; 20],
    samples: [MODSample; 31],
    order_list_length: u8,
    restart_point: u8,
    order_list: [u8; 128],
    _mk: [u8; 4],
    pub patterns: Vec<MODPattern>,
}

type MODPattern = [MODRow; 64];

#[derive(Debug, Clone, Copy)]
pub struct MODColumn {
    pub period: u16,
    pub instrument: u8,
    pub effect: u8,
    pub effect_value: u8,
}

impl Default for MODColumn {
    fn default() -> Self {
        MODColumn {
            period: 0,
            instrument: 0,
            effect: 0,
            effect_value: 0,
        }
    }
}

pub type MODRow = Vec<MODColumn>;

impl Default for MODModule {
    fn default() -> Self {
        MODModule {
            song_name: [0; 20],
            samples: std::array::from_fn(|_| Default::default()),
            order_list_length: 0,
            restart_point: 0,
            order_list: [0; 128],
            _mk: [0; 4],
            patterns: Vec::new(),
        }
    }
}

impl MODModule {
    pub fn load(mut reader: impl io::Read + io::Seek) -> Result<MODModule> {
        let mut module = MODModule::default();
        reader.read(&mut module.song_name)?;
        for sample in &mut module.samples {
            reader.read(&mut sample.sample_name)?;
            sample.length = reader.read_u16::<BigEndian>()?.into();
            sample.length <<= 1;
            sample.finetune = reader.read_i8()?;
            if sample.finetune > 7 {
                sample.finetune -= 16;
            }
            sample.volume = reader.read_u8()?;
            sample.loop_start = reader.read_u16::<BigEndian>()?.into();
            sample.loop_start <<= 1;
            sample.loop_length = reader.read_u16::<BigEndian>()?.into();
            sample.loop_length <<= 1;
        }
        module.order_list_length = reader.read_u8()?;
        module.restart_point = reader.read_u8()?;
        reader.read(&mut module.order_list)?;
        reader.read(&mut module._mk)?;
        let channel_count = module.figure_out_channel_count().expect("file is invalid!");
        // TODO: handle StarTrekker 8 channel!
        // let is_startrekker= if module._mk.starts_with(b"FLT") | module._mk.starts_with(b"EXO") { true } else { false };
        let total_patterns = module.order_list.iter().max().unwrap();
        module.patterns.reserve_exact((total_patterns + 1) as usize);
        for _pattern_index in 0..(*total_patterns + 1) {
            let mut pattern: MODPattern =
                std::array::from_fn(|_| Vec::with_capacity(channel_count));

            for row in &mut pattern {
                *row = (0..channel_count)
                    .map(|_channel_idx| {
                        let mut data = [0; 4]; // Let type inference figure out the number type :)
                        reader.read_exact(&mut data).map(|()| MODColumn {
                            period: (((data[0] & 0x0F) as u16) << 8) | data[1] as u16,
                            instrument: (data[0] & 0xF0) | ((data[2] & 0xF0) >> 4),
                            effect: data[2] & 0x0F,
                            effect_value: data[3],
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            }
            module.patterns.push(pattern);
        }
        for sample in &mut module.samples {
            // Sample is 8 bit
            let mut data: Vec<u8> = Vec::with_capacity(sample.length as usize);
            data.resize((sample.length).try_into()?, 0);
            reader.read_exact(&mut data)?;
            sample.audio = data
                .iter()
                .map(|x| i8::from_ne_bytes([*x]) as i16 * 256)
                .collect();
        }
        Ok(module)
    }

    fn figure_out_channel_count(&self) -> Option<usize> {
        // thanks issotm! :)
        match &self._mk {
            b"M.K." | b"M!K!" => Some(4), // ProTracker (resp. up to 64 patterns, and above that)
            b"N.T." => Some(4),           // NoiseTracker
            b"M&K!" | b"FEST" => Some(4), // "fleg's module train-er"
            b"CD81" => Some(8),           // Octalyser (Atari STe/Falcon)
            b"OKTA" | b"OCTA" => Some(8), // Oktalyzer, OctaMED
            b"FLT4" | b"EXO4" => Some(4), // StarTrekker (4 channels)
            b"FLT8" | b"EXO8" => {
                // StarTrekker (8 channels)
                // order list needs to have all even values,
                // since the patterns are 2 4 channel patterns.
                if self.order_list.iter().all(|&value| value % 2 == 0) {
                    Some(8)
                } else {
                    Some(4) // it's probably really a 4 channel file.
                }
            }
            id if id.starts_with(b"TDZ") => {
                // TakeTracker
                char::from_u32(id[3].into())
                    .unwrap()
                    .to_digit(10)
                    .map(|n| n as usize)
            }
            id if id.ends_with(b"CHN") => {
                // Various
                char::from_u32(id[0].into())
                    .unwrap()
                    .to_digit(10)
                    .map(|n| n as usize)
            }
            id if id.ends_with(b"CH") || id.ends_with(b"CN") => {
                // FastTracker and TakeTracker resp.
                std::str::from_utf8(&id[0..2])
                    .ok()
                    .and_then(|id_str| id_str.parse().ok())
            }
            _ => None,
        }
    }

    fn figure_out_tracker(&self) -> Option<MODTracker> {
        // this is sort of a port of amifilemagic.c by Heikki Orsila and Michael Doering
        // alongside some notes from OpenMPT
        let channel_count = self.figure_out_channel_count()?;

        let mut has_slen_sreplen_zero = 0;
        let mut no_slen_sreplen_zero = 0;
        let mut has_slen_sreplen_one = 0;
        let mut no_slen_sreplen_one = 0;
        let mut no_slen_has_volume = 0;
        let mut finetune_used = false;

        for sample in &self.samples {
            if sample.volume > 64 {
                return None;
            }
            match sample.finetune {
                0 => {}
                1..=15 => finetune_used = true,
                _ => return None,
            }
            if sample.length != 0 && (sample.loop_start + sample.loop_length) > sample.length {
                // repeat length is (likely) in bytes rather than words
                return Some(Soundtracker);
            }
            if sample.loop_start == 0 {
                if sample.length != 0 {
                    // loop length = 0 apparently crashes an Amiga
                    if sample.loop_length == 2 {
                        has_slen_sreplen_one += 1;
                    } else if sample.loop_length == 0 {
                        has_slen_sreplen_zero += 1;
                    }
                } else {
                    if sample.loop_length != 0 {
                        no_slen_sreplen_one += 1;
                    } else {
                        no_slen_sreplen_zero += 1;
                    }
                    if sample.volume != 0 {
                        no_slen_has_volume += 1;
                    }
                }
            }
        }

        let mut pfx: [usize; 32] = [0; 32];
        let mut pfxarg: [usize; 32] = [0; 32];
        let mut lowest_note: u8 = 0;
        let mut highest_note: u8 = 0;
        let amiga_range_notes;

        for pattern in &self.patterns {
            for row in pattern {
                for current_row in row {
                    match find_note_value(current_row.period) {
                        Some(note) => {
                            if note > highest_note {
                                highest_note = note;
                            } else if note < lowest_note {
                                lowest_note = note;
                            }
                        }
                        None => {}
                    }

                    let effect_index = current_row.effect as usize;
                    let fxarg = current_row.effect_value as usize;
                    match effect_index {
                        0 => {
                            if fxarg != 0 {
                                pfx[effect_index] += 1;
                            }
                            if pfxarg[effect_index] > fxarg {
                                pfxarg[effect_index] = fxarg;
                            }
                        }
                        1..=0xD => {
                            pfx[effect_index] += 1;
                            if pfxarg[effect_index] > fxarg {
                                pfxarg[effect_index] = fxarg;
                            }
                        }
                        0xE => {
                            pfx[((fxarg as usize >> 4) & 0xF) + 0x10] += 1;
                        }
                        0xF => {
                            if fxarg > 0x1F {
                                pfx[0xE] += 1;
                            } else {
                                pfx[0xF] += 1;
                            }
                            if pfxarg[0xF] > fxarg {
                                pfxarg[0xF] = fxarg;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        amiga_range_notes = lowest_note >= 12 && highest_note <= 47;

        for j in 0x11..0x1F {
            if pfx[j] != 0 && finetune_used != false {
                if self.restart_point != 0x7F && self.restart_point != 0x78 {
                    return Some(MODTracker::Fasttracker);
                } else {
                    if amiga_range_notes {
                        return Some(MODTracker::Protracker);
                    } else {
                        return Some(MODTracker::ScreamTracker);
                    }
                }
            }
        }

        if self.restart_point == 0x7F
            && has_slen_sreplen_zero <= has_slen_sreplen_one
            && no_slen_sreplen_zero <= no_slen_sreplen_one
        {
            return Some(MODTracker::Protracker);
        }

        if self.restart_point > 0x7F {
            return Some(MODTracker::ProtrackerCompatible);
        }

        if self.restart_point == 0
            && has_slen_sreplen_zero > has_slen_sreplen_one
            && no_slen_sreplen_zero > no_slen_sreplen_one
        {
            if pfx[0x10] == 0 {
                return Some(MODTracker::ProtrackerCompatible);
            }
        }

        if pfx[0x5] != 0 || pfx[0x6] != 0 || pfx[0x7] != 0 || pfx[0x9] != 0 {
            return Some(MODTracker::ProtrackerCompatible);
        }

        if self.restart_point != 0
            && self.restart_point <= self.order_list_length
            && has_slen_sreplen_zero <= has_slen_sreplen_one
            && no_slen_sreplen_zero <= no_slen_sreplen_one
            && no_slen_sreplen_zero == 1
        {
            return Some(MODTracker::Noisetracker);
        }

        if self.restart_point < 0x80
            && has_slen_sreplen_zero <= has_slen_sreplen_one
            && no_slen_sreplen_zero <= no_slen_sreplen_one
            && no_slen_has_volume == 1
        {
            return Some(MODTracker::Noisetracker);
        }

        if self.restart_point < 0x80
            && has_slen_sreplen_zero <= has_slen_sreplen_one
            && no_slen_sreplen_zero >= no_slen_sreplen_one
        {
            return Some(MODTracker::Soundtracker);
        }
        if channel_count != 4 {
            Some(MODTracker::GenericMultichannel)
        } else {
            Some(MODTracker::ProtrackerCompatible)
        }
    }

    fn is_amiga(&self) -> bool {
        match self.figure_out_tracker() {
            Some(MODTracker::Soundtracker) => true,
            Some(MODTracker::Noisetracker) => true,
            Some(MODTracker::Protracker) => true,
            _ => false,
        }
    }
}

impl ModuleInterface for MODModule {
    fn samples(&self) -> Vec<Sample> {
        self.samples
            .iter()
            .map(|s| Sample {
                base_frequency: ((if self.is_amiga() { 8287.0 } else { 8363.0 })
                    * (s.finetune as f64 / 96.0).exp2())
                .round() as u32,
                loop_type: if s.loop_length > 2 {
                    LoopType::Forward
                } else {
                    LoopType::None
                },
                loop_start: s.loop_start,
                loop_end: s.loop_start + s.loop_length,

                default_volume: s.volume,
                global_volume: 64,

                audio: s.audio.clone(),
            })
            .collect()
    }

    fn patterns(&self) -> Vec<Pattern> {
        let mut patterns = Vec::<Pattern>::with_capacity(self.patterns.len());

        for p in &self.patterns {
            let mut pattern = Pattern::with_capacity(64);
            for r in p {
                let mut row = Row::with_capacity(r.len());
                for (i, c) in r.iter().enumerate() {
                    let oc = Column {
                        note: match find_note_value(c.period) {
                            None => Note::None,
                            Some(n) => Note::On(n + 36),
                        },
                        vol: if c.effect == 0xc {
                            VolEffect::Volume(c.effect_value)
                        } else {
                            VolEffect::None
                        },
                        instrument: c.instrument,
                        effect: match c.effect {
                            0x0 => {
                                if c.effect_value != 0 {
                                    Effect::Arpeggio(c.effect_value)
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x1 => {
                                if c.effect_value != 0 {
                                    Effect::PortaUp(std::cmp::min(c.effect_value, 0xDF))
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x2 => {
                                if c.effect_value != 0 {
                                    Effect::PortaDown(std::cmp::min(c.effect_value, 0xDF))
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x3 => Effect::TonePorta(c.effect_value),
                            0x4 => Effect::Vibrato(c.effect_value),
                            0x5 => {
                                if c.effect_value != 0 {
                                    Effect::VolSlideTonePorta(c.effect_value)
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x6 => {
                                if c.effect_value != 0 {
                                    Effect::VolSlideVibrato(c.effect_value)
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x7 => Effect::Tremolo(c.effect_value),
                            0x8 => {
                                if !self.is_amiga() {
                                    Effect::FineSetPan(c.effect_value)
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0x9 => Effect::SampleOffset(c.effect_value),
                            0xA => {
                                if c.effect_value != 0 {
                                    Effect::VolSlide(c.effect_value)
                                } else {
                                    Effect::None(0)
                                }
                            }
                            0xB => Effect::PosJump(c.effect_value),
                            0xC => Effect::None(0),
                            0xD => Effect::PatBreak(c.effect_value),
                            0xE => match c.effect_value & 0xF0 {
                                0x10 => Effect::PortaUp(c.effect_value | 0xf0),
                                0x20 => Effect::PortaDown(c.effect_value | 0xf0),
                                0x30 => Effect::GlissandoControl(c.effect_value & 0x0F > 1),
                                0x40 => Effect::SetVibratoWaveform(c.effect_value & 0x0F),
                                0x60 => match c.effect_value & 0x0F {
                                    0 => Effect::PatLoopStart,
                                    _ => Effect::PatLoop(c.effect_value & 0x0F),
                                },
                                0x70 => Effect::SetTremoloWaveform(c.effect_value & 0x0F),
                                0x80 => Effect::SetPan(c.effect_value & 0x0F),
                                0x90 => Effect::Retrig(c.effect_value & 0x0F),
                                0xA0 => {
                                    if c.effect_value != 0 {
                                        Effect::VolSlide((c.effect_value & 0x0F) << 4 | 0x0F)
                                    } else {
                                        Effect::None(0)
                                    }
                                }
                                0xB0 => {
                                    if c.effect_value != 0 {
                                        Effect::VolSlide((c.effect_value & 0x0F) | 0xF0)
                                    } else {
                                        Effect::None(0)
                                    }
                                }
                                0xC0 => Effect::NoteCut(c.effect_value & 0x0F),
                                0xD0 => Effect::NoteDelay(c.effect_value & 0x0F),
                                0xE0 => Effect::PatDelay(c.effect_value & 0x0F),
                                _ => Effect::None(c.effect_value),
                            },
                            0xF => {
                                if c.effect_value <= 0x1F {
                                    Effect::SetSpeed(c.effect_value)
                                } else {
                                    Effect::SetTempo(c.effect_value)
                                }
                            }
                            _ => Effect::None(c.effect_value),
                        },
                    };

                    row.push(oc)
                }
                pattern.push(row)
            }
            patterns.push(pattern)
        }

        patterns
    }

    fn module(&self) -> Module {
        Module {
            mode: PlaybackMode::MOD(MODOptions {
                amiga: self.is_amiga(),
            }),
            linear_freq_slides: false,
            fast_volume_slides: false,
            initial_tempo: 125,
            initial_speed: 6,
            initial_global_volume: 64,
            mixing_volume: 64,
            samples: self.samples(),
            patterns: self.patterns(),
            playlist: { self.order_list[..self.order_list_length as usize].to_vec() },
            name: String::from_utf8_lossy(&self.song_name)
                .trim_end_matches("\0")
                .to_string(),
        }
    }
}
