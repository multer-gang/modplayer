use super::module::{
    Column, Effect, LoopType, Module, ModuleInterface, Note, Pattern, PlaybackMode, Row, S3MOptions, Sample, VolEffect
};
use byteorder::{LittleEndian, NativeEndian, ReadBytesExt};
use std::{
    io::{self, Read, SeekFrom},
    slice,
};
use anyhow::{Result, anyhow};

#[derive(Debug)]
pub struct S3MModule {
    // FILE STRUCTURE

    song_name: [u8;28],
    _unused: u32,
    order_amount: u16,
    sample_amount: u16,
    pattern_amount: u16,
    flags: u16,
    tracker_metadata: u16,
    ffi: u16,
    _scrm: u32,
    global_volume: u8,
    initial_speed: u8,
    initial_tempo: u8,
    mixing_volume: u8,
    ramping: u8,
    default_panning: u8,
    _unused2: [u8;8],
    special: u16,
    channel_settings: [u8;32],
    orders: Vec<u8>,

    sample_offsets: Vec<u16>,
    pattern_offsets: Vec<u16>,
    channel_panning: [u8;32],

    // PUBLIC
    pub samples: Vec<S3MSample>,
    pub patterns: Vec<S3MPattern>,
}

#[derive(Debug, Default)]
pub struct S3MSample {
    sample_type: u8,
    filename: [u8;12],
    memseg: [u8;3],
    length: u32,
    loop_begin: u32,
    loop_end: u32,
    volume: u8,
    _unused: u8,
    packed: u8,
    flags: u8,
    c4speed: u32,
    _unused2: u32,
    int_gp: u16,
    sample_name: [u8;28],
    _scrs: [u8;4],

    // Public
    pub audio: Vec<i16>,
}

type S3MPattern = [S3MRow;64];

#[derive(Debug, Clone, Copy)]
pub struct S3MColumn {
    pub note: u8,
    pub instrument: u8,
    pub vol: u8,
    pub effect: u8,
    pub effect_value: u8,
}

impl Default for S3MColumn {
    fn default() -> Self {
        S3MColumn {
            note: 255,
            instrument: 0,
            vol: 255,
            effect: 0,
            effect_value: 0,
        }
    }
}

pub type S3MRow = [S3MColumn;32];

impl Default for S3MModule {
    fn default() -> Self {
        // Somebody please fix this monstrosity.
        S3MModule {
            song_name: [0;28],
            _unused: 0,
            order_amount: 0,
            sample_amount: 0,
            pattern_amount: 0,
            flags: 0,
            tracker_metadata: 0,
            ffi: 0,
            _scrm: 0,
            global_volume: 0,
            initial_speed: 0,
            initial_tempo: 0,
            mixing_volume: 0,
            ramping: 0,
            default_panning: 0,
            _unused2: [0;8],
            special: 0,
            channel_settings: [0;32],
            orders: Vec::new(),
            sample_offsets: Vec::new(),
            pattern_offsets: Vec::new(),
            channel_panning: [0;32],
            samples: Vec::new(),
            patterns: Vec::new(),
        }
    }
}

impl S3MModule {
    pub fn load(mut reader: impl io::Read + io::Seek) -> Result<S3MModule> {
        let mut module = S3MModule::default();

        // HEADER START
        reader.read(&mut module.song_name).unwrap();
        module._unused = reader.read_u32::<LittleEndian>().unwrap();
        module.order_amount = reader.read_u16::<LittleEndian>().unwrap();
        module.sample_amount = reader.read_u16::<LittleEndian>().unwrap();
        module.pattern_amount = reader.read_u16::<LittleEndian>().unwrap();
        module.flags = reader.read_u16::<LittleEndian>().unwrap();
        module.tracker_metadata = reader.read_u16::<LittleEndian>().unwrap();
        module.ffi = reader.read_u16::<LittleEndian>().unwrap();
        module._scrm = reader.read_u32::<LittleEndian>().unwrap();
        if module._scrm != 0x4D524353 {
            return Err(anyhow!("File is not a valid module"))
        };
        module.global_volume = reader.read_u8().unwrap();
        module.initial_speed = reader.read_u8().unwrap();
        module.initial_tempo = reader.read_u8().unwrap();
        module.mixing_volume = reader.read_u8().unwrap();
        module.ramping = reader.read_u8().unwrap();
        module.default_panning = reader.read_u8().unwrap();
        reader.read(&mut module._unused2).unwrap();
        module.special = reader.read_u16::<LittleEndian>().unwrap();
        reader.read(&mut module.channel_settings).unwrap();
        module.orders.resize(module.order_amount as usize, 255);
        reader.read(&mut module.orders).unwrap();

        module.sample_offsets.resize(module.sample_amount as usize, 0);
        reader.read_u16_into::<LittleEndian>(&mut module.sample_offsets).unwrap();

        module.pattern_offsets.resize(module.pattern_amount as usize, 0);
        reader.read_u16_into::<LittleEndian>(&mut module.pattern_offsets).unwrap();

        reader.read(&mut module.channel_panning).unwrap();
        // HEADER END

        // SAMPLES START
        for offset in &module.sample_offsets {
            if *offset == 0 {
                module.samples.push(S3MSample::default());
                continue;
            }

            reader.seek(SeekFrom::Start((*offset as u64) << 4)).unwrap();
            let mut sample: S3MSample = S3MSample::default();

            sample.sample_type = reader.read_u8().unwrap();
            if sample.sample_type > 1 {
                return Err(anyhow!("Adlib module detected"))
            }
            reader.read(&mut sample.filename).unwrap();
            reader.read(&mut sample.memseg).unwrap();
            sample.length = reader.read_u32::<LittleEndian>().unwrap();
            sample.loop_begin = reader.read_u32::<LittleEndian>().unwrap();
            sample.loop_end = reader.read_u32::<LittleEndian>().unwrap();
            sample.volume = reader.read_u8().unwrap();
            sample._unused = reader.read_u8().unwrap();
            sample.packed = reader.read_u8().unwrap();
            if sample.packed == 1 {
                return Err(anyhow!("Compressed samples detected"))
            }
            sample.flags = reader.read_u8().unwrap();
            sample.c4speed = reader.read_u32::<LittleEndian>().unwrap();
            reader.seek(SeekFrom::Current(4)).unwrap();
            sample.int_gp = reader.read_u16::<LittleEndian>().unwrap();
            reader.seek(SeekFrom::Current(6)).unwrap();
            reader.read(&mut sample.sample_name).unwrap();

            let sampledata_offset: u32 =
                ((sample.memseg[1] as u32) << 4) |
                ((sample.memseg[2] as u32) << 12) |
                ((sample.memseg[0] as u32) << 20);
            reader.seek(SeekFrom::Start(sampledata_offset as u64)).unwrap();

            if sample.flags & 0b100 != 0 {
                // Sample is 16 bit
                let mut data: Vec<u8> = Vec::with_capacity(sample.length as usize * 2);
                data.resize((sample.length * 2).try_into().unwrap(), 0);
                reader.read_exact(&mut data).unwrap();

                if module.ffi == 1 {
                    // Signed?
                    sample.audio = data
                        .chunks(2)
                        .map(|x| i16::from_le_bytes(x.try_into().unwrap()))
                        .collect();
                } else {
                    sample.audio = data
                        .chunks(2)
                        .map(|x| u16::from_le_bytes(x.try_into().unwrap()) as i16 - 32767)
                        .collect();
                }
            } else {
                // Sample is 8 bit
                let mut data: Vec<u8> = Vec::with_capacity(sample.length as usize);
                data.resize((sample.length).try_into().unwrap(), 0);
                reader.read_exact(&mut data).unwrap();

                if module.ffi == 1 {
                    // Signed?
                    sample.audio = data
                        .iter()
                        .map(|x| i8::from_ne_bytes([*x]) as i16 * 256)
                        .collect();
                } else {
                    sample.audio = data.iter().map(|x| (*x as i16 - 128) * 256).collect();
                }
            }

            module.samples.push(sample);
        }
        // SAMPLES END

        // PATTERNS START
        for offset in &module.pattern_offsets {
            if *offset == 0 {
                module.patterns.push([S3MRow::default();64]);
                continue;
            }

            // println!("Offset: {}", offset);
            reader.seek(SeekFrom::Start(((*offset as u64) << 4) + 2)).unwrap();
            let mut pattern = [S3MRow::default();64];

            let mut row = 0usize;
            let mut channel;
            'unpacking: loop {
                let packed_byte = reader.read_u8().unwrap();
                if packed_byte == 0 {
                    row += 1;
                }
                channel = (packed_byte & 31) as usize;
                if packed_byte & 32 != 0 { // note and instrument in the next 2 bytes
                    pattern[row][channel].note = reader.read_u8().unwrap();
                    pattern[row][channel].instrument = reader.read_u8().unwrap();
                }
                if packed_byte & 64 != 0 { // volume in the next byte
                    pattern[row][channel].vol = reader.read_u8().unwrap();
                }
                if packed_byte & 128 != 0 { // effect in the next 2 bytes
                    pattern[row][channel].effect = reader.read_u8().unwrap();
                    pattern[row][channel].effect_value = reader.read_u8().unwrap();
                }
                if row == 64 {
                    module.patterns.push(pattern);
                    break 'unpacking;
                }
            }

        }

        Ok(module)
    }

    fn is_gus(&self) -> bool {
        let mut total = 0u16;
        for sample in &self.samples {
            if sample.sample_type < 2 {
                total |= sample.int_gp
            };
        };

        match total {
            1 => false,
            0 => self.tracker_metadata > 0x1300,
            _ => true
        }
    }
}

impl ModuleInterface for S3MModule {
    fn samples(&self) -> Vec<Sample> {
        self.samples
            .iter()
            .map(|s| Sample {
                base_frequency: s.c4speed,
                loop_type: if s.flags & 1 != 0 { LoopType::Forward } else { LoopType::None },
                loop_start: s.loop_begin,
                loop_end: s.loop_end,

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
                    if self.channel_settings[i] & 0x7F >= 16 // Ignore AdLib channels
                        // || self.channel_settings[i] == 255 // Also ignore unassigned channels
                        || self.channel_settings[i] & 0x80 != 0 // Also ignore muted channels
                    {
                        continue;
                    }

                    let oc = Column {
                        note: match c.note {
                            255 => Note::None,
                            254 => Note::Cut,
                            _ => {
                                let octave = c.note >> 4;
                                let pitch = c.note & 0xF;

                                Note::On(octave*12+pitch+12)
                            },
                        },
                        instrument: c.instrument,
                        vol: match c.vol {
                            0..=64 => VolEffect::Volume(c.vol),
                            128..=192 => VolEffect::SetPan(c.vol - 128), // Modplug hack
                            _ => VolEffect::None,
                        },
                        effect: match c.effect {
                            1 => Effect::SetSpeed(c.effect_value),
                            2 => Effect::PosJump(c.effect_value),
                            3 => Effect::PatBreak(c.effect_value),
                            4 => Effect::VolSlide(c.effect_value),
                            5 => Effect::PortaDown(c.effect_value),
                            6 => Effect::PortaUp(c.effect_value),
                            7 => Effect::TonePorta(c.effect_value),
                            8 => Effect::Vibrato(c.effect_value),
                            9 => Effect::Tremor(c.effect_value),
                            10 => Effect::Arpeggio(c.effect_value),
                            11 => Effect::VolSlideVibrato(c.effect_value),
                            12 => Effect::VolSlideTonePorta(c.effect_value),
                            13 => Effect::SetChanVol(c.effect_value),
                            14 => Effect::ChanVolSlide(c.effect_value),
                            15 => Effect::SampleOffset(c.effect_value),
                            16 => Effect::PanSlide(c.effect_value),
                            17 => Effect::Retrig(c.effect_value),
                            18 => Effect::Tremolo(c.effect_value),
                            19 => match c.effect_value & 0xF0 {
                                // Sxy
                                0x10 => Effect::GlissandoControl(c.effect_value & 0x0F > 1),
                                0x20 => Effect::SetFinetune(c.effect_value & 0x0F),
                                0x30 => Effect::SetVibratoWaveform(c.effect_value & 0x0F),
                                0x40 => Effect::SetTremoloWaveform(c.effect_value & 0x0F),
                                0x50 => Effect::SetPanbrelloWaveform(c.effect_value & 0x0F),
                                0x60 => Effect::FinePatternDelay(c.effect_value & 0x0F),
                                0x80 => Effect::SetPan(c.effect_value & 0x0F),
                                0x90 => Effect::SoundControl(c.effect_value & 0x0F),
                                0xA0 => Effect::HighOffset(c.effect_value & 0x0F),
                                0xB0 => match c.effect_value & 0x0F {
                                    0 => Effect::PatLoopStart,
                                    _ => Effect::PatLoop(c.effect_value & 0x0F),
                                },
                                0xC0 => Effect::NoteCut(c.effect_value & 0x0F),
                                0xD0 => Effect::NoteDelay(c.effect_value & 0x0F),
                                0xE0 => Effect::PatDelay(c.effect_value & 0x0F),
                                0xF0 => Effect::SetActiveMacro(c.effect_value & 0x0F),

                                _ => Effect::None(c.effect_value),
                            },
                            20 => match c.effect_value & 0xF0 {
                                0x0 => Effect::DecTempo(c.effect_value & 0x0F),
                                0x1 => Effect::IncTempo(c.effect_value & 0x0F),
                                _ => Effect::SetTempo(c.effect_value),
                            },
                            21 => Effect::FineVibrato(c.effect_value),
                            22 => Effect::SetGlobalVol(c.effect_value),
                            23 => Effect::GlobalVolSlide(c.effect_value),
                            24 => Effect::FineSetPan(c.effect_value),
                            25 => Effect::Panbrello(c.effect_value),
                            26 => Effect::MIDIMacro(c.effect_value),

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
            mode: PlaybackMode::S3M(S3MOptions { gus: self.is_gus() }),
            linear_freq_slides: false,
            fast_volume_slides: self.tracker_metadata == 0x1300 || self.flags & 0x40 != 0,
            initial_tempo: self.initial_tempo,
            initial_speed: self.initial_speed,
            initial_global_volume: 64,
            mixing_volume: if self.is_gus() { 48 } else { self.mixing_volume & 0x7F },
            samples: self.samples(),
            patterns: self.patterns(),
            playlist: self.orders.clone(),
            name: String::from_utf8_lossy(&self.song_name)
                .trim_end_matches("\0")
                .to_string(),
        }
    }
}
