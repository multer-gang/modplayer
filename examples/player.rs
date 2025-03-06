use modplayer::format_it::ITModule;
use modplayer::format_s3m::S3MModule;
use modplayer::player::{Interpolation, Player};

use modplayer::module::ModuleInterface;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "Rust module player")]
#[command(about = "Very barebones tracker module player (IT samples only for now)")]
struct Args {
    file: String,

    #[arg(short, long, value_enum, default_value_t = Interpolation::Linear)]
    interpolation: Interpolation,

    #[arg(short, long, default_value_t = 0)]
    position: u8,
}

fn main() {
    let args = Args::parse();

    let file = std::fs::File::open(args.file).unwrap();
    let module: S3MModule = S3MModule::load(file).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });
    let binding = module.module();

    let mut player: Player = Player::from_module(&binding, 48000);
    player.interpolation = args.interpolation;
    player.current_position = args.position;
    player.current_pattern = player.module.playlist[player.current_position as usize];

    if player.current_pattern == 254 {
        println!("Selected pattern is a separator, skipping.");
        loop {
            if player.current_pattern == 254 {
                player.current_position += 1;
                player.current_pattern = player.module.playlist[player.current_position as usize];
            } else {
                break;
            }
        }
    }

    let sdl_context = sdl2::init().unwrap();
    let audio_subsystem = sdl_context.audio().unwrap();

    let spec = sdl2::audio::AudioSpecDesired {
        freq: Some(48000),
        channels: Some(1),
        samples: Some(512),
    };

    let device = audio_subsystem
        .open_playback(None, &spec, |_| player)
        .unwrap();

    println!("Module name: {}", binding.name);
    device.resume();

    ctrlc::set_handler(move || std::process::exit(0)).expect("error listening to interrupt");

    loop {}
}

// fn format_note(note: u8) -> String {
//     match note {
//         120 => return "...".to_string(),
//         121..=253 => return "Fde".to_string(),
//         254 => return "Cut".to_string(),
//         255 => return "Off".to_string(),
//         _ => {}
//     }

//     let mut out = String::new();

//     out.push_str(match note % 12 {
//         0 => "C-",
//         1 => "C#",
//         2 => "D-",
//         3 => "D#",
//         4 => "E-",
//         5 => "F-",
//         6 => "F#",
//         7 => "G-",
//         8 => "G#",
//         9 => "A-",
//         10 => "A#",
//         11 => "B-",
//         _ => unreachable!()
//     });

//     out.push_str(format!("{}", note/12).as_str());

//     out
// }

// fn format_col(row: &engine::format_it::ITColumn) -> String {
//     let instrument = if row.instrument == 0 { "..".to_string() } else { format!("{:0>2}", row.instrument) };
//     let volume = if row.vol == 255 { "..".to_string() } else { format!("{:0>2}", row.vol) };
//     let fx = if row.effect == 0 { ".".to_string() } else { format!("{}", (0x40+row.effect) as char) };
//     let fxvalue = if row.effect_value == 0 { if row.effect != 0 { "00".to_string() } else { "..".to_string() } } else { format!("{:0>2X}", row.effect_value) };

//     format!("{} {instrument} {volume} {fx}{fxvalue}", format_note(row.note))
// }

// fn main() {
//     let file = std::fs::File::open("/home/polyzium/Downloads/chris31b.it").unwrap();
//     let module: ITModule = ITModule::load(file).unwrap();
//     // let binding = module.module();

//     // let player: Player = Player::from_module(&binding);
//     // println!("{:?}", player.module.samples[0]);

//     println!("\n{}\n", String::from_utf8_lossy(&module.song_name).trim_end_matches(0 as char));

//     for (i, p) in module.patterns.iter().enumerate() {
//         println!("Pattern {}", i);
//         for (i, r) in p.rows.iter().enumerate() {
//             print!("{:0>2} | ", i);
//             for cr in r {
//                 print!("{} | ", format_col(cr))
//             }
//             print!("\n")
//         }
//         print!("\n")
//     }
// }
