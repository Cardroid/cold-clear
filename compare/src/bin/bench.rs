//! Measures raw search throughput: plays a fixed-seed solo game, performing a fixed number of
//! think cycles before each move, and reports think cycles per second.
//!
//! Usage: `bench [moves] [thinks-per-move] [seed]`

use std::time::{Duration, Instant};

use cold_clear::evaluation::Standard;
use libtetris::Board;
use rand::prelude::*;
use rand::rngs::StdRng;

fn main() {
    let mut args = std::env::args().skip(1);
    let moves: u32 = args.next().map_or(200, |s| s.parse().unwrap());
    let thinks: u32 = args.next().map_or(2000, |s| s.parse().unwrap());
    let seed: u64 = args.next().map_or(0, |s| s.parse().unwrap());

    let mut rng = StdRng::seed_from_u64(seed);
    let eval = Standard::default();

    let mut board = Board::new();
    for _ in 0..6 {
        board.add_next_piece(board.generate_next_piece(&mut rng));
    }
    let mut bot = cold_clear::BotState::new(board.clone(), Default::default());

    let mut think_time = Duration::default();
    let mut total_thinks = 0u64;
    let mut played = 0;
    let mut attack = 0;
    let mut lines = 0;
    for _ in 0..moves {
        let start = Instant::now();
        for _ in 0..thinks {
            match bot.think() {
                Ok(thinker) => {
                    bot.finish_thinking(thinker.think(&eval));
                    total_thinks += 1;
                }
                Err(_) => break,
            }
        }
        think_time += start.elapsed();

        let (mv, _) = match bot.suggest_move(&eval, None, 0) {
            Some(v) => v,
            None => break,
        };
        let next = board.advance_queue().unwrap();
        if mv.hold && board.hold(next).is_none() {
            board.advance_queue();
        }
        let lock = board.lock_piece(mv.expected_location);
        attack += lock.garbage_sent;
        lines += lock.cleared_lines.len();
        bot.advance_move(mv.expected_location);
        let next = board.generate_next_piece(&mut rng);
        board.add_next_piece(next);
        bot.add_next_piece(next);
        played += 1;
        if lock.locked_out {
            break;
        }
    }

    println!(
        "moves {} thinks {} time {:.3}s -> {:.0} thinks/s | attack {} lines {}",
        played,
        total_thinks,
        think_time.as_secs_f64(),
        total_thinks as f64 / think_time.as_secs_f64(),
        attack,
        lines
    );
}
