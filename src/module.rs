use std::{fmt::Display};

#[derive(Default, Debug, Clone, Copy)]
pub enum Note {
    #[default]
    None,
    On(u8),
    Fade,
    Cut,
    Off,
}

#[derive(Debug)]
pub enum Effect {
    // Based off ITs set
    None(u8),

    SetSpeed(u8),          // Axx
    PosJump(u8),           // Bxx
    PatBreak(u8),          // Cxx
    VolSlide(u8),          // Dxy
    PortaDown(u8),         // Exx
    PortaUp(u8),           // Fxx
    TonePorta(u8),         // Gxx
    Vibrato(u8),           // Hxy
    Tremor(u8),            // Ixy
    Arpeggio(u8),          // Jxy
    VolSlideVibrato(u8),   // Kxy
    VolSlideTonePorta(u8), // Lxy
    SetChanVol(u8),        // Mxx
    ChanVolSlide(u8),      // Nxy
    SampleOffset(u8),      // Oxx
    PanSlide(u8),          // Pxy
    Retrig(u8),            // Qxy
    Tremolo(u8),           // Rxy

    GlissandoControl(bool),   // S1x
    SetFinetune(u8),          // S2x
    SetVibratoWaveform(u8),   // S3x
    SetTremoloWaveform(u8),   // S4x
    SetPanbrelloWaveform(u8), // S5x
    FinePatternDelay(u8),     // S6x

    PastNoteCut,     // S70
    PastNoteOff,     // S71
    PastNoteFade,    // S72
    NNANoteCut,      // S73
    NNANoteContinue, // S74
    NNANoteOff,      // S75
    NNANoteFade,     // S76
    VolEnvOff,       // S77
    VolEnvOn,        // S78
    PanEnvOff,       // S79
    PanEnvOn,        // S7A
    PitchEnvOff,     // S7B
    PitchEnvOn,      // S7C

    SetPan(u8),         // S8x
    SoundControl(u8),   // S9x
    HighOffset(u8),     // SAx
    PatLoopStart,       // SB0
    PatLoop(u8),        // SBx
    NoteCut(u8),        // SCx
    NoteDelay(u8),      // SDx
    PatDelay(u8),       // SEx
    SetActiveMacro(u8), // SFx

    DecTempo(u8),       // T0x
    IncTempo(u8),       // T1x
    SetTempo(u8),       // Txx
    FineVibrato(u8),    // Uxy
    SetGlobalVol(u8),   // Vxx
    GlobalVolSlide(u8), // Wxy
    FineSetPan(u8),     // Xxx
    Panbrello(u8),      // Yxy
    MIDIMacro(u8),      // Zxx
                        // SmoothMIDIMacro(u8) // \xx, ModPlug hack
}

impl Default for Effect {
    fn default() -> Self {
        Self::None(0)
    }
}

#[derive(Default, Debug)]
pub enum VolEffect {
    // Based off IT's set
    #[default]
    None,

    FineVolSlideUp(u8),   // a0x
    FineVolSlideDown(u8), // b0x
    VolSlideUp(u8),       // c0x
    VolSlideDown(u8),     // d0x
    PortaDown(u8),        // e0x
    PortaUp(u8),          // f0x
    TonePorta(u8),        // g0x
    VibratoDepth(u8),     // h0x
    SetPan(u8),           // pxx
    Volume(u8),           // vxx
}

#[derive(Debug, Clone)]
pub enum LoopType {
    None,
    Forward,
    PingPong,
}

#[derive(Debug)]
pub enum PlaybackMode {
    MOD,
    S3M(S3MOptions),
    XM,
    IT,
    ITSample,
}

#[derive(Debug)]
pub struct S3MOptions {
    pub gus: bool
}

#[derive(Debug, Clone)]
pub struct Sample {
    pub base_frequency: u32, // freq @ C-5
    pub loop_type: LoopType,
    pub loop_start: u32,
    pub loop_end: u32,

    pub default_volume: u8,
    pub global_volume: u8,
    // TODO: sustain loops

    // TODO: vibrato
    pub audio: Vec<i16>,
}

pub type Pattern = Vec<Row>;
pub type Row = Vec<Column>;

#[derive(Debug)]
pub struct Column {
    pub note: Note,
    pub instrument: u8,
    pub vol: VolEffect,
    pub effect: Effect,
}

#[derive(Debug)]
pub struct Module {
    pub name: String,
    pub mode: PlaybackMode,

    pub linear_freq_slides: bool,
    pub fast_volume_slides: bool,
    pub initial_tempo: u8,
    pub initial_speed: u8,
    pub initial_global_volume: u8,

    pub mixing_volume: u8,

    pub samples: Vec<Sample>,
    //TODO: instruments
    pub patterns: Vec<Pattern>,
    pub playlist: Vec<u8>,
}

pub trait ModuleInterface {
    fn samples(&self) -> Vec<Sample>;
    // TODO: instruments
    fn patterns(&self) -> Vec<Pattern>;

    fn module(&self) -> Module;
}
