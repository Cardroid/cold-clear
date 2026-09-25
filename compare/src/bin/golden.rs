//! Prints a digest of move generation and evaluation results over many pseudo-random boards.
//! Used to check that performance-only changes do not alter the bot's behaviour: the output
//! must be byte-for-byte identical before and after such a change.
//!
//! Usage: `golden [boards] [seed]`

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use cold_clear::evaluation::{Evaluator, Standard};
use libtetris::*;
use rand::prelude::*;
use rand::rngs::StdRng;

fn main() {
    let mut args = std::env::args().skip(1);
    let boards: u32 = args.next().map_or(3000, |s| s.parse().unwrap());
    let seed: u64 = args.next().map_or(0, |s| s.parse().unwrap());
    let verbose = std::env::var_os("GOLDEN_VERBOSE").is_some();

    let mut rng = StdRng::seed_from_u64(seed);
    let eval = Standard::default();
    let fast_eval = Standard::fast_config();
    // Only rewards T slots, to check that the corpus exercises T slot detection.
    let tslot_eval = Standard {
        tslot: [1, 10, 100, 1000],
        ..zeroed()
    };
    let baseline = tslot_eval.evaluate(
        &Board::<u16>::new().lock_piece(FallingPiece {
            kind: PieceState(Piece::O, RotationState::North),
            x: 0,
            y: 0,
            tspin: TspinStatus::None,
        }),
        &Board::new(),
        0,
        Piece::O,
    );
    let baseline = format!("{:?}", baseline.0);
    let mut tslot_hits = 0u64;
    let mut digest = DefaultHasher::new();
    let modes = [
        MovementMode::ZeroG,
        MovementMode::ZeroGComplete,
        MovementMode::TwentyG,
        MovementMode::HardDropOnly,
    ];

    let mut board = Board::<u16>::new();
    let mut placements = 0u64;
    for i in 0..boards {
        if board.column_heights().iter().any(|&h| h > 18) || rng.gen_bool(0.01) {
            board = Board::new();
        }
        if rng.gen_bool(0.05) {
            let col = rng.gen_range(0, 10);
            for _ in 0..rng.gen_range(1, 4) {
                board.add_garbage(col);
            }
        }
        board.b2b_bonus = rng.gen_bool(0.5);
        board.combo = rng.gen_range(0, 4);
        board.hold_piece = if rng.gen_bool(0.5) {
            Some(Piece::I)
        } else {
            None
        };
        while board.next_queue().count() < 3 {
            board.add_next_piece(board.generate_next_piece(&mut rng));
        }

        let mode = modes[i as usize % modes.len()];
        let mut chosen = None;
        for &piece in &[
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ] {
            let spawned = match SpawnRule::Row19Or20.spawn(piece, &board) {
                Some(s) => s,
                None => continue,
            };
            let mut moves = find_moves(&board, spawned, mode);
            moves.sort_by_key(|p| {
                (
                    p.location.kind.0 as u8,
                    p.location.kind.1 as u8,
                    p.location.x,
                    p.location.y,
                    p.location.tspin as u8,
                )
            });
            for (j, mv) in moves.iter().enumerate() {
                let mut result = board.clone();
                let lock = result.lock_piece(mv.location);
                let (value, reward) = eval.evaluate(&lock, &result, mv.inputs.time, piece);
                let line = format!(
                    "{} {:?} {:?} {} {:?} {:?} {:?} {:?}",
                    i, mode, mv.location, mv.inputs.time, mv.inputs.movements, lock, value, reward
                );
                if verbose {
                    println!("{}", line);
                }
                line.hash(&mut digest);
                let fast = fast_eval.evaluate(&lock, &result, mv.inputs.time, piece);
                format!("{:?}", fast).hash(&mut digest);
                let tslot = format!(
                    "{:?}",
                    tslot_eval.evaluate(&lock, &result, mv.inputs.time, piece).0
                );
                if tslot != baseline {
                    tslot_hits += 1;
                }
                tslot.hash(&mut digest);
                format!("{:?}", result).hash(&mut digest);
                placements += 1;
                if piece == board.get_next_piece().unwrap() && rng.gen_range(0, moves.len()) == j {
                    chosen = Some(mv.location);
                }
            }
        }
        let next = board.advance_queue().unwrap();
        match chosen {
            Some(loc) if loc.kind.0 == next => {
                board.lock_piece(loc);
            }
            _ => board = Board::new(),
        }
    }
    println!(
        "placements {} tslot {} digest {:016x}",
        placements,
        tslot_hits,
        digest.finish()
    );
}

fn zeroed() -> Standard {
    Standard {
        back_to_back: 0,
        bumpiness: 0,
        bumpiness_sq: 0,
        row_transitions: 0,
        height: 0,
        top_half: 0,
        top_quarter: 0,
        jeopardy: 0,
        cavity_cells: 0,
        cavity_cells_sq: 0,
        overhang_cells: 0,
        overhang_cells_sq: 0,
        covered_cells: 0,
        covered_cells_sq: 0,
        tslot: [0; 4],
        well_depth: 0,
        max_well_depth: 0,
        well_column: [0; 10],
        b2b_clear: 0,
        clear1: 0,
        clear2: 0,
        clear3: 0,
        clear4: 0,
        tspin1: 0,
        tspin2: 0,
        tspin3: 0,
        mini_tspin1: 0,
        mini_tspin2: 0,
        perfect_clear: 0,
        combo_garbage: 0,
        move_time: 0,
        wasted_t: 0,
        ..Standard::default()
    }
}
