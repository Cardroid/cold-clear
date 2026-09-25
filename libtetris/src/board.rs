use std::iter::DoubleEndedIterator;

use arrayvec::ArrayVec;
use enumset::EnumSet;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

use crate::*;

#[derive(Debug, Serialize, Deserialize)]
pub struct Board<R = u16> {
    cells: ArrayVec<[R; 40]>,
    column_heights: [i32; 10],
    pub combo: u32,
    pub b2b_bonus: bool,
    pub hold_piece: Option<Piece>,
    /// Stored inline so that cloning a board (which happens for every generated placement)
    /// doesn't need to allocate. Longer queues spill to the heap.
    next_pieces: SmallVec<[Piece; 16]>,
    pub bag: EnumSet<Piece>,
}

impl<R: Row> Clone for Board<R> {
    fn clone(&self) -> Self {
        // ArrayVec's Clone impl copies element by element, which is noticeably slow for a type
        // that is cloned as often as this one. The rows are Copy, so copy them in bulk instead.
        let mut cells = ArrayVec::new();
        cells.try_extend_from_slice(&self.cells).unwrap();
        Board {
            cells,
            column_heights: self.column_heights,
            combo: self.combo,
            b2b_bonus: self.b2b_bonus,
            hold_piece: self.hold_piece,
            next_pieces: self.next_pieces.clone(),
            bag: self.bag,
        }
    }
}

pub trait Row: Copy + Clone + 'static {
    fn set(&mut self, x: usize, color: CellColor);
    fn get(&self, x: usize) -> bool;

    /// Returns whether any of the cells in `mask` (bit `x` for column `x`) are filled.
    #[inline]
    fn intersects(&self, mask: u16) -> bool {
        (0..10).any(|x| mask & 1 << x != 0 && self.get(x))
    }

    fn is_full(&self) -> bool;
    fn is_empty(&self) -> bool;
    fn cell_color(&self, x: usize) -> CellColor;

    const EMPTY: &'static Self;
    const SOLID: &'static Self;
}

impl<R: Row> Board<R> {
    /// Creates a blank board with an empty queue.
    pub fn new() -> Self {
        Board {
            cells: [*R::EMPTY; 40].into(),
            column_heights: [0; 10],
            combo: 0,
            b2b_bonus: false,
            hold_piece: None,
            next_pieces: SmallVec::new(),
            bag: EnumSet::all(),
        }
    }

    /// Creates a board with existing field, remain pieces in the bag, hold piece, back-to-back status and combo count.
    pub fn new_with_state(
        field: [[bool; 10]; 40],
        bag_remain: EnumSet<Piece>,
        hold: Option<Piece>,
        b2b: bool,
        combo: u32,
    ) -> Self {
        let mut board = Board {
            cells: [*R::EMPTY; 40].into(),
            column_heights: [0; 10],
            combo: combo,
            b2b_bonus: b2b,
            hold_piece: hold,
            next_pieces: SmallVec::new(),
            bag: if bag_remain.is_empty() {
                EnumSet::all()
            } else {
                bag_remain
            },
        };
        board.set_field(field);
        board
    }

    /// Randomly selects a piece from the bag.
    ///
    /// This function does not remove the generated piece from the bag.
    /// Use add_next_piece() to add it to the queue.
    pub fn generate_next_piece(&self, rng: &mut impl rand::Rng) -> Piece {
        use rand::prelude::*;
        let choices: ArrayVec<[_; 7]> = self.bag.iter().collect();
        *choices.choose(rng).unwrap()
    }

    /// Retrieves the next piece in the queue.
    ///
    /// If the queue is empty, returns the set of possible next pieces.
    pub fn get_next_piece(&self) -> Result<Piece, EnumSet<Piece>> {
        self.next_pieces.first().copied().ok_or(self.bag)
    }

    /// Retrieves the piece after the next piece in the queue if it is known.
    pub fn get_next_next_piece(&self) -> Option<Piece> {
        self.next_pieces.get(1).copied()
    }

    /// Adds the piece to the next queue and removes it from the bag.
    ///
    /// If the bag becomes empty, the bag is refilled.
    pub fn add_next_piece(&mut self, piece: Piece) {
        self.bag.remove(piece);
        if self.bag.is_empty() {
            self.bag = EnumSet::all();
        }
        self.next_pieces.push(piece);
    }

    fn remove_cleared_lines(&mut self) -> ArrayVec<[i32; 4]> {
        let mut cleared = ArrayVec::new();
        // Fast path: most placements don't clear any lines.
        if !self.cells.iter().any(|r| r.is_full()) {
            return cleared;
        }
        let mut lineno = 0;
        self.cells.retain(|r| {
            let full = r.is_full();
            if full {
                cleared.push(lineno);
            }
            lineno += 1;
            !full
        });

        for _ in 0..cleared.len() {
            self.cells.push(*R::EMPTY);
        }
        for x in 0..10 {
            self.column_heights[x] -= cleared.len() as i32;
            while self.column_heights[x] > 0
                && !self.cells[self.column_heights[x] as usize - 1].get(x)
            {
                self.column_heights[x] -= 1;
            }
        }
        cleared
    }

    #[inline]
    pub fn occupied(&self, x: i32, y: i32) -> bool {
        // casting to unsigned turns negative coordinates into out of range ones
        x as u32 >= 10 || y as u32 >= 40 || self.cells[y as usize].get(x as usize)
    }

    pub fn get_row(&self, y: i32) -> &R {
        if y < 0 {
            R::SOLID
        } else if y >= 40 {
            R::EMPTY
        } else {
            &self.cells[y as usize]
        }
    }

    pub fn set_cell_color(&mut self, x: i32, y: i32, color: CellColor) {
        self.cells[y as usize].set(x as usize, color);
        let h = &mut self.column_heights[x as usize];
        if color != CellColor::Empty {
            *h = (*h).max(y + 1);
        } else if *h == y + 1 {
            while *h > 0 && !self.cells[*h as usize - 1].get(x as usize) {
                *h -= 1;
            }
        }
    }

    pub fn obstructed(&self, piece: &FallingPiece) -> bool {
        // Equivalent to checking `occupied` for each cell, but tests a whole row of the piece at
        // once using a precomputed mask.
        let shape = &SHAPE_MASKS[piece.kind.index()];
        let left = piece.x + shape.min_dx;
        let bottom = piece.y + shape.min_dy;
        if left < 0 || piece.x + shape.max_dx >= 10 || bottom < 0 || piece.y + shape.max_dy >= 40 {
            return true;
        }
        for i in 0..shape.height {
            if self.cells[(bottom + i) as usize].intersects(shape.rows[i as usize] << left) {
                return true;
            }
        }
        false
    }

    pub fn above_stack(&self, piece: &FallingPiece) -> bool {
        piece
            .cells()
            .iter()
            .all(|&(x, y)| y >= self.column_heights[x as usize])
    }

    pub fn on_stack(&self, piece: &FallingPiece) -> bool {
        piece.cells().iter().any(|&(x, y)| self.occupied(x, y - 1))
    }

    /// Does all logic associated with locking a piece.
    ///
    /// Clears lines, detects clear kind, calculates garbage, maintains combo and back-to-back
    /// state, detects perfect clears, detects lockout.
    pub fn lock_piece(&mut self, piece: FallingPiece) -> LockResult {
        let mut locked_out = true;
        for &(x, y) in &piece.cells() {
            self.cells[y as usize].set(x as usize, piece.kind.0.color());
            if self.column_heights[x as usize] < y + 1 {
                self.column_heights[x as usize] = y + 1;
            }
            if y < 20 {
                locked_out = false;
            }
        }
        let cleared = self.remove_cleared_lines();

        let placement_kind = PlacementKind::get(cleared.len(), piece.tspin);

        let mut garbage_sent = placement_kind.garbage();

        let mut did_b2b = false;
        if placement_kind.is_clear() {
            if placement_kind.is_hard() {
                if self.b2b_bonus {
                    garbage_sent += 1;
                    did_b2b = true;
                }
                self.b2b_bonus = true;
            } else {
                self.b2b_bonus = false;
            }

            if self.combo as usize >= COMBO_GARBAGE.len() {
                garbage_sent += COMBO_GARBAGE.last().unwrap();
            } else {
                garbage_sent += COMBO_GARBAGE[self.combo as usize];
            }

            self.combo += 1;
        } else {
            self.combo = 0;
        }

        let perfect_clear = self.column_heights == [0; 10];
        if perfect_clear {
            garbage_sent = 10;
        }

        let l = LockResult {
            placement_kind,
            garbage_sent,
            perfect_clear,
            locked_out,
            combo: if self.combo == 0 {
                None
            } else {
                Some(self.combo - 1)
            },
            b2b: did_b2b,
            cleared_lines: cleared,
        };

        l
    }

    /// Holds the passed piece, returning the previous hold piece.
    ///
    /// If there is a piece in hold, it is returned.
    pub fn hold(&mut self, piece: Piece) -> Option<Piece> {
        let hold = self.hold_piece;
        self.hold_piece = Some(piece);
        hold
    }

    pub fn next_queue<'a>(&'a self) -> impl DoubleEndedIterator<Item = Piece> + 'a {
        self.next_pieces.iter().copied()
    }

    /// Returns the piece that should be spawned, or None if the queue is empty.
    pub fn advance_queue(&mut self) -> Option<Piece> {
        if self.next_pieces.is_empty() {
            None
        } else {
            Some(self.next_pieces.remove(0))
        }
    }

    pub fn column_heights(&self) -> &[i32; 10] {
        &self.column_heights
    }

    pub fn add_garbage(&mut self, col: usize) -> bool {
        let mut row = *R::EMPTY;
        for x in 0..10 {
            if x == col {
                if self.column_heights[x] != 0 {
                    self.column_heights[x] += 1;
                }
            } else {
                row.set(x, CellColor::Garbage);
                self.column_heights[x] += 1;
            }
        }
        let dead = self.cells.pop().map_or(false, |r| !r.is_empty());
        self.cells.insert(0, row);
        dead
    }

    pub fn to_compressed(&self) -> Board {
        Board {
            cells: self
                .cells
                .iter()
                .map(|r| {
                    let mut row = 0;
                    for x in 0..10 {
                        row.set(x, r.cell_color(x));
                    }
                    row
                })
                .collect(),
            b2b_bonus: self.b2b_bonus,
            combo: self.combo,
            column_heights: self.column_heights,
            next_pieces: self.next_pieces.clone(),
            hold_piece: self.hold_piece,
            bag: self.bag,
        }
    }

    pub fn set_field(&mut self, field: [[bool; 10]; 40]) {
        self.cells.clear();
        self.column_heights = [0; 10];
        for y in 0..40 {
            let mut r = *R::EMPTY;
            for x in 0..10 {
                if field[y][x] {
                    r.set(x, CellColor::Garbage);
                    self.column_heights[x] = y as i32 + 1;
                }
            }
            self.cells.push(r)
        }
    }

    pub fn get_field(&self) -> [[bool; 10]; 40] {
        let mut field = [[false; 10]; 40];
        for y in 0..40 {
            for x in 0..10 {
                field[y][x] = self.occupied(x as i32, y as i32)
            }
        }
        field
    }

    pub fn next_bag(&self) -> EnumSet<Piece> {
        let mut bag = self.bag;
        for p in self.next_queue().rev() {
            if bag == EnumSet::all() {
                bag = EnumSet::empty();
            }
            bag.insert(p);
        }
        bag
    }
}

/// Row masks of a piece shape, relative to its lowest row and leftmost column.
struct ShapeMasks {
    min_dx: i32,
    max_dx: i32,
    min_dy: i32,
    max_dy: i32,
    height: i32,
    rows: [u16; 4],
}

const fn shape_masks(cells: [(i32, i32); 4]) -> ShapeMasks {
    let mut min_dx = cells[0].0;
    let mut max_dx = cells[0].0;
    let mut min_dy = cells[0].1;
    let mut max_dy = cells[0].1;
    let mut i = 1;
    while i < 4 {
        let (x, y) = cells[i];
        if x < min_dx {
            min_dx = x;
        }
        if x > max_dx {
            max_dx = x;
        }
        if y < min_dy {
            min_dy = y;
        }
        if y > max_dy {
            max_dy = y;
        }
        i += 1;
    }
    let mut rows = [0; 4];
    let mut i = 0;
    while i < 4 {
        let (x, y) = cells[i];
        rows[(y - min_dy) as usize] |= 1 << (x - min_dx);
        i += 1;
    }
    ShapeMasks {
        min_dx,
        max_dx,
        min_dy,
        max_dy,
        height: max_dy - min_dy + 1,
        rows,
    }
}

static SHAPE_MASKS: [ShapeMasks; 28] = {
    const EMPTY: ShapeMasks = ShapeMasks {
        min_dx: 0,
        max_dx: 0,
        min_dy: 0,
        max_dy: 0,
        height: 0,
        rows: [0; 4],
    };
    let mut table = [EMPTY; 28];
    let mut i = 0;
    while i < 28 {
        table[i] = shape_masks(PIECE_CELLS[i]);
        i += 1;
    }
    table
};

impl Row for u16 {
    #[inline]
    fn intersects(&self, mask: u16) -> bool {
        *self & mask != 0
    }

    fn set(&mut self, x: usize, color: CellColor) {
        if color == CellColor::Empty {
            *self &= !(1 << x);
        } else {
            *self |= 1 << x;
        }
    }

    #[inline]
    fn get(&self, x: usize) -> bool {
        *self & (1 << x) != 0
    }

    fn is_full(&self) -> bool {
        self == Self::SOLID
    }

    fn is_empty(&self) -> bool {
        self == Self::EMPTY
    }

    fn cell_color(&self, x: usize) -> CellColor {
        if self.get(x) {
            CellColor::Garbage
        } else {
            CellColor::Empty
        }
    }

    const SOLID: &'static u16 = &0b11111_11111;
    const EMPTY: &'static u16 = &0;
}

#[derive(Copy, Clone, Debug)]
pub struct ColoredRow([CellColor; 10]);

impl Default for ColoredRow {
    fn default() -> Self {
        ColoredRow([CellColor::Empty; 10])
    }
}

impl Row for ColoredRow {
    fn set(&mut self, x: usize, color: CellColor) {
        self.0[x] = color;
    }

    fn get(&self, x: usize) -> bool {
        self.0[x] != CellColor::Empty
    }

    fn is_full(&self) -> bool {
        self.0.iter().all(|&c| c != CellColor::Empty)
    }

    fn cell_color(&self, x: usize) -> CellColor {
        self.0[x]
    }

    fn is_empty(&self) -> bool {
        self.0.iter().all(|&c| c == CellColor::Empty)
    }

    const SOLID: &'static Self = &ColoredRow([CellColor::Unclearable; 10]);
    const EMPTY: &'static Self = &ColoredRow([CellColor::Empty; 10]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::prelude::*;

    fn check_obstructed<R: Row>(board: &Board<R>) {
        for &p in &[
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ] {
            for &r in &[
                RotationState::North,
                RotationState::East,
                RotationState::South,
                RotationState::West,
            ] {
                for x in -4..14 {
                    for y in -4..44 {
                        let piece = FallingPiece {
                            kind: PieceState(p, r),
                            x,
                            y,
                            tspin: TspinStatus::None,
                        };
                        let expected = piece.cells().iter().any(|&(x, y)| board.occupied(x, y));
                        assert_eq!(board.obstructed(&piece), expected, "{:?}", piece);
                    }
                }
            }
        }
    }

    #[test]
    fn obstructed_matches_cell_check() {
        let mut rng = StdRng::seed_from_u64(0);
        for _ in 0..50 {
            let mut field = [[false; 10]; 40];
            let height = rng.gen_range(0, 40);
            for row in field.iter_mut().take(height) {
                for cell in row.iter_mut() {
                    *cell = rng.gen_bool(0.6);
                }
            }
            let mut board = Board::<u16>::new();
            board.set_field(field);
            check_obstructed(&board);
            let mut colored = Board::<ColoredRow>::new();
            colored.set_field(field);
            check_obstructed(&colored);
        }
    }

    #[test]
    fn clone_is_equal() {
        let mut board = Board::<u16>::new();
        board.add_garbage(3);
        board.add_next_piece(Piece::T);
        board.hold_piece = Some(Piece::I);
        board.combo = 2;
        let clone = board.clone();
        assert_eq!(format!("{:?}", board), format!("{:?}", clone));
    }
}
