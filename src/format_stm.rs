use super::module::{
    Column, Effect, LoopType, Module, ModuleInterface, Note, Pattern, PlaybackMode, Row,
    S3MOptions, Sample, VolEffect,
};
use anyhow::{anyhow, Result};
use byteorder::{LittleEndian, NativeEndian, ReadBytesExt};
use std::{
    io::{self, Read, SeekFrom},
    slice,
};
use crate::stm_tools::calculate_stm_tempo;

fn translate_early_tempo(tempo: u8) -> u8 {
    ((tempo / 10) << 4) + (tempo % 10)
}

#[derive(Debug)]
pub struct STMModule {
    // FILE STRUCTURE
    song_name: [u8; 20],
    tracker_name: [u8; 8],
    dos_eof: u8,
    file_type: u8,
    version_major: u8,
    version_minor: u8,
    initial_tempo: u8,
    pattern_amount: u8,
    global_volume: u8,
    orders: Vec<u8>,
    // PUBLIC
    pub samples: Vec<STMSample>,
    pub patterns: Vec<STMPattern>,
}

#[derive(Debug, Default)]
pub struct STMSample {
    filename: [u8; 12],
    memseg: u16,
    length: u16,
    loop_begin: u16,
    loop_end: u16,
    volume: u8,
    c4speed: u16,
    // Public
    pub audio: Vec<i16>,
}

type STMPattern = [STMRow; 64];

#[derive(Debug, Clone, Copy)]
pub struct STMColumn {
    pub note: u8,
    pub instrument: u8,
    pub vol: u8,
    pub effect: u8,
    pub effect_value: u8,
}

impl Default for STMColumn {
    fn default() -> Self {
        STMColumn {
            note: 255,
            instrument: 0,
            vol: 255,
            effect: 0,
            effect_value: 0,
        }
    }
}

pub type STMRow = [STMColumn; 4];

impl Default for STMModule {
    fn default() -> Self {
        STMModule {
            song_name: [0; 20],
            tracker_name: [0; 8],
            dos_eof: 0,
            file_type: 0,
            version_major: 0,
            version_minor: 0,
            initial_tempo: 0,
            pattern_amount: 0,
            global_volume: 0,
            orders: Vec::new(),
            samples: Vec::new(),
            patterns: Vec::new(),
        }
    }
}

impl STMModule {
    pub fn load(mut reader: impl io::Read + io::Seek) -> Result<STMModule> {
        let mut module = STMModule::default();

        // HEADER START
        reader.read(&mut module.song_name)?;
        reader.read(&mut module.tracker_name)?;
        module.dos_eof = reader.read_u8()?;
        for letter in module.tracker_name {
            // I don't feel like being particularly strict here...
            if !letter.is_ascii() {
                return Err(anyhow!("File is not a valid module"));
            }
        }
        if module.dos_eof != 0x02 && module.dos_eof != 0x1A {
            return Err(anyhow!("Invalid end of ID"));
        }
        // 1 = song, does not have samples
        // 2 = module, has samples
        // 2 == better. :)
        module.file_type = reader.read_u8()?;
        match module.file_type {
            1 => return Err(anyhow!("STM songs are not supported")),
            2 => {}
            _ => return Err(anyhow!("Invalid STM file type")),
        }
        module.version_major = reader.read_u8()?;
        module.version_minor = reader.read_u8()?;
        if module.version_major != 2
            && !(module.version_minor == 0
                || module.version_minor == 10
                || module.version_minor == 20
                || module.version_minor == 21)
        {
            return Err(anyhow!("Invalid STM version"));
        }
        module.initial_tempo = reader.read_u8()?;
        module.pattern_amount = reader.read_u8()?;
        module.global_volume = reader.read_u8()?;
        if module.version_minor < 21 {
            module.initial_tempo = translate_early_tempo(module.initial_tempo);
        }
        if module.initial_tempo == 0 {
            module.initial_tempo = 0x60;
        }
        // HEADER END

        // SAMPLES START
        for _i in 0..31 {
            let mut sample: STMSample = STMSample::default();
            reader.seek(SeekFrom::Start(48 + (32 * _i)))?;
            reader.read(&mut sample.filename)?;
            reader.seek(SeekFrom::Current(2))?;
            sample.memseg = reader.read_u16::<LittleEndian>()?;
            sample.length = reader.read_u16::<LittleEndian>()?;
            sample.loop_begin = reader.read_u16::<LittleEndian>()?;
            sample.loop_end = reader.read_u16::<LittleEndian>()?;
            sample.volume = reader.read_u8()?;
            reader.seek(SeekFrom::Current(1))?;
            sample.c4speed = reader.read_u16::<LittleEndian>()?;
            reader.seek(SeekFrom::Current(6))?;

            if sample.volume != 0 {
                let sampledata_offset = (sample.memseg as u64) << 4;
                reader
                    .seek(SeekFrom::Start(sampledata_offset as u64))
                    ?;

                // Sample is 8 bit
                let mut data: Vec<u8> = Vec::with_capacity(sample.length as usize);
                data.resize((sample.length).try_into()?, 0);
                // normally a bad idea but some STMs have samples whose lengths go beyond the end of the file!
                reader.read_exact(&mut data).ok();

                sample.audio = data
                    .iter()
                    .map(|x| i8::from_ne_bytes([*x]) as i16 * 256)
                    .collect();
            }

            module.samples.push(sample);
        }
        // SAMPLES END

        reader.seek(SeekFrom::Start(0x410))?;
        for i in 0..128 {
            let byte = reader.read_u8()?;
            // Any patterns above 63 is undefined behavior
            if byte < 63 {
                module.orders.push(byte);
            }
        }
        module.orders.push(0xFF);

        // PATTERNS START
        reader.seek(SeekFrom::Start(0x490))?;
        for _offset in 0..module.pattern_amount {
            let mut pattern = [STMRow::default(); 64];

            let mut row = 0usize;
            'unpacking: loop {
                for channel in 0..4 {
                    let packed_byte = reader.read_u8()?;
                    let current_row = &mut pattern[row][channel];
                    match packed_byte {
                        0xFB => {
                            current_row.note = 0;
                            current_row.instrument = 0;
                            current_row.vol = 0;
                            current_row.effect = 0;
                            current_row.effect_value = 0;
                        }
                        0xFD => {
                            current_row.note = 254;
                        }
                        0xFC => {}
                        _ => {
                            let packed_byte2 = reader.read_u8()?;
                            let packed_byte3 = reader.read_u8()?;
                            let packed_byte4 = reader.read_u8()?;
                            current_row.note = packed_byte;
                            current_row.instrument = packed_byte2 >> 3;
                            current_row.vol =
                                (packed_byte2 & 7) | ((packed_byte3 & 0xF0) >> 1);
                            current_row.effect = packed_byte3 & 0x0F;
                            current_row.effect_value = packed_byte4;
                            if module.version_minor < 21 && current_row.effect == 1 {
                                current_row.effect_value =
                                    translate_early_tempo(packed_byte4);
                            }
                        }
                    }
                }
                row += 1;
                if row == 64 {
                    module.patterns.push(pattern);
                    break 'unpacking;
                }
            }
        }

        Ok(module)
    }
}

impl ModuleInterface for STMModule {
    fn samples(&self) -> Vec<Sample> {
        self.samples
            .iter()
            .map(|s| Sample {
                base_frequency: s.c4speed as u32,
                loop_type: if s.loop_end < 0xFFFF {
                    LoopType::Forward
                } else {
                    LoopType::None
                },
                loop_start: s.loop_begin as u32,
                loop_end: s.loop_end as u32,

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
                for (_i, c) in r.iter().enumerate() {
                    let oc = Column {
                        note: match c.note {
                            255 => Note::None,
                            254 => Note::Cut,
                            _ => {
                                let octave = (c.note >> 4) + 2;
                                let pitch = c.note & 0xF;

                                Note::On(octave * 12 + pitch + 12)
                            }
                        },
                        instrument: c.instrument,
                        vol: match c.vol {
                            0..=64 => VolEffect::Volume(c.vol),
                            _ => VolEffect::None,
                        },
                        effect: match c.effect {
                            1 => Effect::STMTempo(c.effect_value),
                            2 => Effect::PosJump(c.effect_value),
                            3 => Effect::PatBreak(c.effect_value),
                            4 => {
                                if c.effect_value != 0 {
                                    Effect::VolSlide(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            5 => {
                                if c.effect_value != 0 {
                                    Effect::PortaDown(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            6 => {
                                if c.effect_value != 0 {
                                    Effect::PortaUp(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            7 => {
                                if c.effect_value != 0 {
                                    Effect::TonePorta(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            8 => {
                                if c.effect_value != 0 {
                                    Effect::Vibrato(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            9 => {
                                if c.effect_value != 0 {
                                    Effect::Tremor(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
                                }
                            }
                            10 => {
                                if c.effect_value != 0 {
                                    Effect::Arpeggio(c.effect_value)
                                } else {
                                    Effect::None(c.effect_value)
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
            
            mode: PlaybackMode::S3M(S3MOptions { gus: false }),
            linear_freq_slides: false,
            fast_volume_slides: false,
            initial_tempo: calculate_stm_tempo(self.initial_tempo),
            initial_speed: self.initial_tempo >> 4,
            initial_global_volume: self.global_volume,
            mixing_volume: 48,
            samples: self.samples(),
            patterns: self.patterns(),
            playlist: self.orders.clone(),
            name: String::from_utf8_lossy(&self.song_name)
                .trim_end_matches("\0")
                .to_string(),
        }
    }
}
