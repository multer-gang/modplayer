use super::module::{
    Column, Effect, LoopType, Module, ModuleInterface, Note, Pattern, PlaybackMode, Row, Sample,
    VolEffect,
};
use byteorder::{LittleEndian, ReadBytesExt};
use std::{
    io::{self, Read, SeekFrom},
    slice,
};
use anyhow::{anyhow, Result};

#[derive(Debug)]
pub struct ITModule {
    // FILE STRUCTURE
    // Values that usually never go beyond 255 but take two bytes are u16 LE.

    /*0000*/
    _impm: [u8; 4],
    pub song_name: [u8; 26],
    /*001E*/ _pattern_highlight: u16, // Editor only, irrelevant
    /*0020*/ order_amount: u16,
    instrument_amount: u16,
    sample_amount: u16,
    pattern_amount: u16,
    tracker_id: u16,
    format_id: u16,
    flags: u16,
    special: u16,
    /*0030*/ global_volume: u8,
    mixing_volume: u8,
    initial_speed: u8,
    initial_tempo: u8,
    separation: u8,
    pitch_wheel_depth: u8,
    message_length: u16,
    message_offset: u32,
    _reserved: [u8; 4],
    /*0040*/ channel_pan: [u8; 64],

    /*0080*/ channel_volume: [u8; 64],

    /*00C0*/
    orders: Vec<u8>, // song length is order_amount-1 and terminated with 0xFF, ModPlug modules will have order_amount (OrdNum) of 256u16, beware!

    /*xxxx (Offsets)*/
    instrument_offsets: Vec<u32>,
    sample_offsets: Vec<u32>,
    pattern_offsets: Vec<u32>,

    // PUBLIC
    pub instruments: Vec<ITInstrument>,
    pub samples: Vec<ITSample>,
    pub patterns: Vec<ITPattern>,
}

#[derive(Debug, Default)]
pub struct ITInstrument {
    /*0000*/
    _impi: [u8; 4],
    filename: [u8; 12],

    /*0010*/
    _00h: u8, // is that for padding?
    new_note_action: u8,
    duplicate_check_type: u8,
    duplicate_check_action: u8,
    fadeout: u16,
    pitch_pan_sepraration: i8,
    pitch_pan_center: u8,
    global_volume: u8,
    default_pan: u8,
    random_volume: u8,
    random_pan: u8,
    _tracker_version: u16,  // Instrument file only, irrelevant
    _number_of_samples: u8, // Instrument file only, irrelevant
    _x: u8,                 // WTF?

    /*0020-0039*/ instrument_name: [u8; 26],

    /*003A-003F*/
    initial_filter_cutoff: u8,
    initial_filter_resonance: u8,
    midi_channel: u8,
    midi_program: u8,
    midi_bank: u16,

    /*0040*/ note_sample_table: Vec<ITNoteSamplePair>,

    /*0130*/ envelopes: [ITEnvelope; 3],
}

#[derive(Debug, Default)]
pub struct ITNoteSamplePair {
    note: u8,
    sample: u8,
}

#[derive(Debug, Default)]
pub struct ITEnvelope {
    flag: u8,
    node_amount: u8,
    loop_begin: u8,
    loop_end: u8,
    sustain_loop_begin: u8,
    sustain_loop_end: u8,

    nodes: Vec<ITEnvelopeNode>,
}

#[derive(Debug, Default)]
pub struct ITEnvelopeNode {
    y: u8,
    tick: u16,
}

#[derive(Debug, Default)]
pub struct ITSample {
    /*0000*/ _imps: [u8; 4],
    filename: [u8; 12],
    /*0010*/
    _00h: u8,
    global_volume: u8,
    flags: u8,
    volume: u8,
    sample_name: [u8; 26],

    /*0020*/
    convert: u8,
    default_pan: u8,

    /*0030*/
    length: u32,
    loop_begin: u32,
    loop_end: u32,
    c5_speed: u32,

    /*0040*/
    sustain_loop_begin: u32,
    sustain_loop_end: u32,
    sample_pointer: u32,

    vibrato_speed: u8,
    vibrato_depth: u8,
    vibrato_rate: u8,
    vibrato_type: u8,

    // Public
    pub audio: Vec<i16>,
}

#[derive(Debug, Default)]
pub struct ITPattern {
    length: u16,
    rows_amount: u16,
    _x: [u8; 4], // padding?

    pub rows: ITRow,
}

#[derive(Debug)]
pub struct ITColumn {
    pub note: u8,
    pub instrument: u8,
    pub vol: u8,
    pub effect: u8,
    pub effect_value: u8,
}

impl Default for ITColumn {
    fn default() -> Self {
        ITColumn {
            note: 120,
            instrument: 0,
            vol: 255,
            effect: 0,
            effect_value: 0,
        }
    }
}

pub type ITRow = Vec<Vec<ITColumn>>;

impl ITPattern {
    pub fn parse_packed_bytes(&mut self, pattern_bytes: &mut &[u8]) {
        let mut channel_variable = 0u8; // channelvariable according to ITTECH.TXT
        let mut max_channel = 0u8;

        let mut last_notes = [119u8; 64];
        let mut last_instruments = [0u8; 64];
        let mut last_volumes = [255u8; 64];
        let mut last_fx = [0u8; 64];
        let mut last_fxvalues = [0u8; 64];

        let mut masks = [0u8; 64];

        let mut row = Vec::<ITColumn>::with_capacity(64);
        row.resize_with(64, ITColumn::default);

        let mut column = ITColumn::default();

        while self.rows.len() != self.rows_amount.into() {
            pattern_bytes
                .read_exact(slice::from_mut(&mut channel_variable))
                .unwrap();
            if channel_variable != 0 {
                let channel_number = ((channel_variable - 1) & 63) as usize;
                if (max_channel as usize) < channel_number + 1 {
                    max_channel = channel_number as u8 + 1
                };

                if channel_variable & 128 != 0 {
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut masks[channel_number]))
                        .unwrap();
                }

                if masks[channel_number] & 1 != 0 {
                    // Note
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut column.note))
                        .unwrap();
                    last_notes[channel_number] = column.note
                }

                if masks[channel_number] & 2 != 0 {
                    // Instrument
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut column.instrument))
                        .unwrap();
                    last_instruments[channel_number] = column.instrument
                }

                if masks[channel_number] & 4 != 0 {
                    // Volume column
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut column.vol))
                        .unwrap();
                    last_volumes[channel_number] = column.vol
                }

                if masks[channel_number] & 8 != 0 {
                    // Command/Effect column
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut column.effect))
                        .unwrap();
                    pattern_bytes
                        .read_exact(slice::from_mut(&mut column.effect_value))
                        .unwrap();
                    last_fx[channel_number] = column.effect;
                    last_fxvalues[channel_number] = column.effect_value;
                }

                if masks[channel_number] & 16 != 0 {
                    // Last note
                    column.note = last_notes[channel_number]
                }

                if masks[channel_number] & 32 != 0 {
                    // Last instrument
                    column.instrument = last_instruments[channel_number]
                }

                if masks[channel_number] & 64 != 0 {
                    // Last volume
                    column.vol = last_volumes[channel_number]
                }

                if masks[channel_number] & 128 != 0 {
                    // Last command and value
                    column.effect = last_fx[channel_number];
                    column.effect_value = last_fxvalues[channel_number];
                }

                // row.push(channel_row);
                row[channel_number] = column;
                column = ITColumn::default();
            } else {
                // End of row
                // println!("Channels: {}", max_channel);

                row.truncate(max_channel.into());
                self.rows.push(row);

                row = Vec::<ITColumn>::with_capacity(64);
                row.resize_with(64, ITColumn::default);
                // println!("NEXT ROW");
            }
        }
        // println!("PATTERN END");
    }
}

impl Default for ITModule {
    fn default() -> Self {
        // Somebody please fix this monstrosity.
        ITModule {
            // Header
            _impm: [0; 4],
            song_name: [0; 26],
            _pattern_highlight: 0,
            order_amount: 0,
            instrument_amount: 0,
            sample_amount: 0,
            pattern_amount: 0,
            tracker_id: 0,
            format_id: 0,
            flags: 0,
            special: 0,
            global_volume: 0,
            mixing_volume: 0,
            initial_speed: 0,
            initial_tempo: 0,
            separation: 0,
            pitch_wheel_depth: 0,
            message_length: 0,
            message_offset: 0,
            _reserved: [0; 4],
            channel_pan: [0; 64],
            channel_volume: [0; 64],
            orders: Vec::<u8>::new(),
            instrument_offsets: Vec::<u32>::new(),
            sample_offsets: Vec::<u32>::new(),
            pattern_offsets: Vec::<u32>::new(),

            // Data
            instruments: Vec::<ITInstrument>::new(),
            samples: Vec::<ITSample>::new(),
            patterns: Vec::<ITPattern>::new(),
        }
    }
}

impl ITModule {
    pub fn load(mut reader: impl io::Read + io::Seek) -> Result<ITModule> {
        let mut module = ITModule::default();

        // TODO: migrate to byteorder/nom instead of this fucking mess

        // --- HEADER START ---
        // 0000
        reader.read_exact(&mut module._impm).unwrap();
        match std::str::from_utf8(&module._impm).unwrap() {
            "IMPM" => {}
            _ => return Err(anyhow!("File is not a valid module")),
        };
        reader.read_exact(&mut module.song_name).unwrap();

        // 0010
        let mut philigt_buf = [0u8; 2];
        reader.read_exact(&mut philigt_buf).unwrap();
        module._pattern_highlight = u16::from_le_bytes(philigt_buf);

        // 0020
        let mut ordnum_buf = [0u8; 2];
        reader.read_exact(&mut ordnum_buf).unwrap();
        module.order_amount = u16::from_le_bytes(ordnum_buf);
        let mut insnum_buf = [0u8; 2];
        reader.read_exact(&mut insnum_buf).unwrap();
        module.instrument_amount = u16::from_le_bytes(insnum_buf);
        let mut smpnum_buf = [0u8; 2];
        reader.read_exact(&mut smpnum_buf).unwrap();
        module.sample_amount = u16::from_le_bytes(smpnum_buf);
        let mut ptnnum_buf = [0u8; 2];
        reader.read_exact(&mut ptnnum_buf).unwrap();
        module.pattern_amount = u16::from_le_bytes(ptnnum_buf);
        let mut trackerid_buf = [0u8; 2];
        reader.read_exact(&mut trackerid_buf).unwrap();
        module.tracker_id = u16::from_le_bytes(trackerid_buf);
        let mut formatid_buf = [0u8; 2];
        reader.read_exact(&mut formatid_buf).unwrap();
        module.format_id = u16::from_le_bytes(formatid_buf);
        let mut flags_buf = [0u8; 2];
        reader.read_exact(&mut flags_buf).unwrap();
        module.flags = u16::from_le_bytes(flags_buf);
        let mut special_buf = [0u8; 2];
        reader.read_exact(&mut special_buf).unwrap();
        module.special = u16::from_le_bytes(special_buf);

        // 0030
        let mut gv_buf = [0u8];
        reader.read_exact(&mut gv_buf).unwrap();
        module.global_volume = gv_buf[0];
        let mut mv_buf = [0u8];
        reader.read_exact(&mut mv_buf).unwrap();
        module.mixing_volume = mv_buf[0];
        let mut is_buf = [0u8];
        reader.read_exact(&mut is_buf).unwrap();
        module.initial_speed = is_buf[0];
        let mut it_buf = [0u8];
        reader.read_exact(&mut it_buf).unwrap();
        module.initial_tempo = it_buf[0];
        let mut sep_buf = [0u8];
        reader.read_exact(&mut sep_buf).unwrap();
        module.separation = sep_buf[0];
        let mut pwd_buf = [0u8];
        reader.read_exact(&mut pwd_buf).unwrap();
        module.pitch_wheel_depth = pwd_buf[0];
        let mut msglgth_buf = [0u8; 2];
        reader.read_exact(&mut msglgth_buf).unwrap();
        module.message_length = u16::from_le_bytes(msglgth_buf);
        let mut msgoffset_buf = [0u8; 4];
        reader.read_exact(&mut msgoffset_buf).unwrap();
        module.message_offset = u32::from_le_bytes(msgoffset_buf);
        reader.read_exact(&mut module._reserved).unwrap();

        // 0040
        reader.read_exact(&mut module.channel_pan).unwrap();

        // 0080
        reader.read_exact(&mut module.channel_volume).unwrap();

        // 00C0
        module.orders.resize(module.order_amount as usize, 0);
        reader.read_exact(&mut module.orders).unwrap();

        // xxxx (Offsets)
        // Instruments
        let mut io_buf = Vec::<u8>::with_capacity((module.instrument_amount * 4) as usize);
        io_buf.resize((module.instrument_amount * 4) as usize, 0);
        reader.read_exact(&mut io_buf).unwrap();
        module.instrument_offsets = io_buf
            .chunks(4)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<u32>>();

        // Samples
        let mut so_buf = Vec::<u8>::with_capacity((module.sample_amount * 4) as usize);
        so_buf.resize((module.sample_amount * 4) as usize, 0);
        reader.read_exact(&mut so_buf).unwrap();
        module.sample_offsets = so_buf
            .chunks(4)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<u32>>();

        // Patterns
        let mut po_buf = Vec::<u8>::with_capacity((module.pattern_amount * 4) as usize);
        po_buf.resize((module.pattern_amount * 4) as usize, 0);
        reader.read_exact(&mut po_buf).unwrap();
        module.pattern_offsets = po_buf
            .chunks(4)
            .map(|x| u32::from_le_bytes(x.try_into().unwrap()))
            .collect::<Vec<u32>>();
        // --- HEADER END ---

        // --- INSTRUMENTS START ---
        for offset in &module.instrument_offsets {
            reader.seek(SeekFrom::Start(*offset as u64)).unwrap();
            let mut instrument = ITInstrument::default();

            // 0000
            reader.read_exact(&mut instrument._impi).unwrap();
            reader.read_exact(&mut instrument.filename).unwrap();

            // 0010
            instrument._00h = reader.read_u8().unwrap();
            instrument.new_note_action = reader.read_u8().unwrap();
            instrument.duplicate_check_type = reader.read_u8().unwrap();
            instrument.duplicate_check_action = reader.read_u8().unwrap();
            instrument.fadeout = reader.read_u16::<LittleEndian>().unwrap();
            instrument.pitch_pan_sepraration = reader.read_i8().unwrap();
            instrument.pitch_pan_center = reader.read_u8().unwrap();
            instrument.global_volume = reader.read_u8().unwrap();
            instrument.default_pan = reader.read_u8().unwrap();
            instrument.random_volume = reader.read_u8().unwrap();
            instrument.random_pan = reader.read_u8().unwrap();
            instrument._tracker_version = reader.read_u16::<LittleEndian>().unwrap();
            instrument._number_of_samples = reader.read_u8().unwrap();
            instrument._x = reader.read_u8().unwrap();

            // 0020
            reader.read_exact(&mut instrument.instrument_name).unwrap();

            // 0030
            instrument.initial_filter_cutoff = reader.read_u8().unwrap();
            instrument.initial_filter_resonance = reader.read_u8().unwrap();
            instrument.midi_channel = reader.read_u8().unwrap();
            instrument.midi_program = reader.read_u8().unwrap();
            instrument.midi_bank = reader.read_u16::<LittleEndian>().unwrap();

            // 0040
            for _ in 0..120 {
                // 240 bytes
                let mut pair = ITNoteSamplePair::default();

                pair.note = reader.read_u8().unwrap();
                pair.sample = reader.read_u8().unwrap();
                instrument.note_sample_table.push(pair);
            }

            // 0130, 0182, 01D4
            for i in 0..3 as usize {
                let mut env = ITEnvelope::default();

                env.flag = reader.read_u8().unwrap();
                env.node_amount = reader.read_u8().unwrap();
                env.loop_begin = reader.read_u8().unwrap();
                env.loop_end = reader.read_u8().unwrap();
                env.sustain_loop_begin = reader.read_u8().unwrap();
                env.sustain_loop_end = reader.read_u8().unwrap();

                for _ in 0..env.node_amount {
                    let mut node = ITEnvelopeNode::default();

                    node.y = reader.read_u8().unwrap();
                    node.tick = reader.read_u16::<LittleEndian>().unwrap();
                    env.nodes.push(node);
                }

                instrument.envelopes[i] = env;
            }

            module.instruments.push(instrument)
        }
        // --- INSTRUMENTS END ---

        // --- SAMPLES START ---
        for offset in module.sample_offsets.as_slice() {
            reader.seek(SeekFrom::Start(*offset as u64)).unwrap();
            let mut sample = ITSample::default();

            // 0000
            reader.read_exact(&mut sample._imps).unwrap();
            reader.read_exact(&mut sample.filename).unwrap();

            // 0010
            sample._00h = reader.read_u8().unwrap();
            sample.global_volume = reader.read_u8().unwrap();
            sample.flags = reader.read_u8().unwrap();
            sample.volume = reader.read_u8().unwrap();

            reader.read_exact(&mut sample.sample_name).unwrap();

            // 0020
            sample.convert = reader.read_u8().unwrap();
            sample.default_pan = reader.read_u8().unwrap();

            // 0030
            sample.length = reader.read_u32::<LittleEndian>().unwrap();
            sample.loop_begin = reader.read_u32::<LittleEndian>().unwrap();
            sample.loop_end = reader.read_u32::<LittleEndian>().unwrap();
            sample.c5_speed = reader.read_u32::<LittleEndian>().unwrap();

            // 0040
            sample.sustain_loop_begin = reader.read_u32::<LittleEndian>().unwrap();
            sample.sustain_loop_end = reader.read_u32::<LittleEndian>().unwrap();
            sample.sample_pointer = reader.read_u32::<LittleEndian>().unwrap();

            sample.vibrato_speed = reader.read_u8().unwrap();
            sample.vibrato_depth = reader.read_u8().unwrap();
            sample.vibrato_rate = reader.read_u8().unwrap();
            sample.vibrato_type = reader.read_u8().unwrap();

            // Data
            reader
                .seek(SeekFrom::Start(sample.sample_pointer as u64))
                .unwrap();

            if sample.flags & 0b1000 == 0 {
            if sample.flags & 0b10 != 0 {
                // Sample is 16 bit
                let mut data: Vec<u8> = Vec::with_capacity(sample.length as usize * 2);
                data.resize((sample.length * 2).try_into().unwrap(), 0);
                reader.read_exact(&mut data).unwrap();

                if sample.convert & 0b1 != 0 {
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

                if sample.convert & 0b1 != 0 {
                    // Signed?
                    sample.audio = data
                        .iter()
                        .map(|x| i8::from_ne_bytes([*x]) as i16 * 256)
                        .collect();
                } else {
                    sample.audio = data.iter().map(|x| (*x as i16 - 128) * 256).collect();
                }
            }
            }
            // println!("Sample {} length: {}", module.samples.len()+1, sample.audio.len());
            module.samples.push(sample)
        }
        // --- SAMPLES END ---

        // --- PATTERNS START ---
        for offset in module.pattern_offsets.as_slice() {
            if *offset == 0 {
                module.patterns.push(ITPattern::default());
                continue;
            }

            // println!("Offset: {}", offset);
            reader.seek(SeekFrom::Start(*offset as u64)).unwrap();
            let mut pattern = ITPattern::default();

            pattern.length = reader.read_u16::<LittleEndian>().unwrap();
            pattern.rows_amount = reader.read_u16::<LittleEndian>().unwrap();
            // println!("Rows: {}", pattern.rows_amount);
            reader.read_exact(&mut pattern._x).unwrap(); // skip padding(?)

            let mut pattern_bytes = Vec::<u8>::with_capacity(pattern.length.into());
            pattern_bytes.resize(pattern.length.into(), 0);
            reader.read_exact(&mut pattern_bytes).unwrap();

            pattern.parse_packed_bytes(&mut pattern_bytes.as_slice());

            module.patterns.push(pattern);
        }
        // --- PATTERNS END

        Ok(module)
    }
}

impl ModuleInterface for ITModule {
    fn samples(&self) -> Vec<Sample> {
        self.samples
            .iter()
            .map(|s| Sample {
                base_frequency: s.c5_speed,
                loop_type: match s.flags & 0b01010000 {
                    16 => LoopType::Forward,
                    80 => LoopType::PingPong,
                    _ => LoopType::None,
                },
                loop_start: s.loop_begin,
                loop_end: s.loop_end,

                default_volume: s.volume,
                global_volume: s.global_volume,

                audio: s.audio.clone(),
            })
            .collect()
    }

    fn patterns(&self) -> Vec<Pattern> {
        let mut patterns = Vec::<Pattern>::with_capacity(self.patterns.len());

        for p in &self.patterns {
            let mut pattern = Pattern::with_capacity(p.rows_amount.into());
            for r in &p.rows {
                let mut row = Row::with_capacity(r.len());
                for c in r {
                    let oc = Column {
                        note: match c.note {
                            120 => Note::None,
                            121..=253 => Note::Fade,
                            254 => Note::Cut,
                            255 => Note::Off,
                            _ => Note::On(c.note),
                        },
                        instrument: c.instrument,
                        vol: match c.vol {
                            0..=64 => VolEffect::Volume(c.vol),
                            65..=74 => VolEffect::FineVolSlideUp(c.vol - 65),
                            75..=84 => VolEffect::FineVolSlideDown(c.vol - 75),
                            85..=94 => VolEffect::VolSlideUp(c.vol - 85),
                            95..=104 => VolEffect::VolSlideDown(c.vol - 95),
                            105..=114 => VolEffect::PortaDown(c.vol - 105),
                            115..=124 => VolEffect::PortaUp(c.vol - 115),
                            128..=192 => VolEffect::SetPan(c.vol - 128),
                            193..=202 => VolEffect::TonePorta(c.vol - 193),
                            203..=212 => VolEffect::VibratoDepth(c.vol - 203),
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
                                0x70 => match c.effect_value & 0x0F {
                                    // S7x
                                    0x0 => Effect::PastNoteCut,
                                    0x1 => Effect::PastNoteOff,
                                    0x2 => Effect::PastNoteFade,
                                    0x3 => Effect::NNANoteCut,
                                    0x4 => Effect::NNANoteContinue,
                                    0x5 => Effect::NNANoteOff,
                                    0x6 => Effect::NNANoteFade,
                                    0x7 => Effect::VolEnvOff,
                                    0x8 => Effect::VolEnvOn,
                                    0x9 => Effect::PanEnvOff,
                                    0xA => Effect::PanEnvOn,
                                    0xB => Effect::PitchEnvOff,
                                    0xC => Effect::PitchEnvOn,
                                    _ => Effect::None(c.effect_value),
                                },
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
            mode: if self.flags & 0b100 != 0 {
                // Bit 2: On = Use instruments, Off = Use samples.
                PlaybackMode::IT
            } else {
                PlaybackMode::ITSample
            },
            linear_freq_slides: self.flags & 0b1000 != 0, // Bit 3: On = Linear slides, Off = Amiga slides.
            fast_volume_slides: false,
            initial_tempo: self.initial_tempo,
            initial_speed: self.initial_speed,
            initial_global_volume: self.global_volume,
            mixing_volume: self.mixing_volume,
            samples: self.samples(),
            patterns: self.patterns(),
            playlist: self.orders.clone(),
            name: String::from_utf8_lossy(&self.song_name)
                .trim_end_matches("\0")
                .to_string(),
        }
    }
}
