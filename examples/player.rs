use modplayer::load_module;
use modplayer::player::{Interpolation, Player};

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "Rust module player")]
#[command(about = "Very barebones tracker module player (IT/S3M/STM)")]
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
    let binding = load_module(file).unwrap_or_else(|e| {
        eprintln!("{}", e);
        std::process::exit(1)
    });

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
