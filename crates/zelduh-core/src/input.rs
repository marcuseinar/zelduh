//! Per-player input.
//!
//! Input is the only thing that enters the simulation from outside, which is
//! what makes lockstep multiplayer possible: the same input stream replayed
//! against the same seed always produces the same world.

/// Button bits. Matches the Game Boy layout plus a pause bit.
pub mod button {
    pub const UP: u16 = 1 << 0;
    pub const DOWN: u16 = 1 << 1;
    pub const LEFT: u16 = 1 << 2;
    pub const RIGHT: u16 = 1 << 3;
    /// Sword / confirm, the "A" button.
    pub const A: u16 = 1 << 4;
    /// Secondary item, the "B" button.
    pub const B: u16 = 1 << 5;
    pub const START: u16 = 1 << 6;
    pub const SELECT: u16 = 1 << 7;

    pub const DPAD: u16 = UP | DOWN | LEFT | RIGHT;
}

/// One frame of input for one player.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Input {
    /// Buttons held this frame.
    pub buttons: u16,
    /// Buttons held on the previous frame, used for edge detection.
    pub prev: u16,
}

impl Input {
    pub const NONE: Input = Input {
        buttons: 0,
        prev: 0,
    };

    /// Builds an input frame for a fresh press state.
    pub const fn new(buttons: u16) -> Self {
        Input { buttons, prev: 0 }
    }

    /// Advances to a new button state, keeping the previous one for edges.
    pub fn advance(&mut self, buttons: u16) {
        self.prev = self.buttons;
        self.buttons = buttons;
    }

    /// True while the button is held.
    #[inline]
    pub fn held(&self, b: u16) -> bool {
        self.buttons & b != 0
    }

    /// True only on the frame the button goes down.
    #[inline]
    pub fn pressed(&self, b: u16) -> bool {
        self.buttons & b != 0 && self.prev & b == 0
    }

    /// True only on the frame the button comes up.
    #[inline]
    pub fn released(&self, b: u16) -> bool {
        self.buttons & b == 0 && self.prev & b != 0
    }

    /// Horizontal axis as -1, 0 or 1.
    pub fn axis_x(&self) -> i32 {
        match (self.held(button::LEFT), self.held(button::RIGHT)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        }
    }

    /// Vertical axis as -1, 0 or 1.
    pub fn axis_y(&self) -> i32 {
        match (self.held(button::UP), self.held(button::DOWN)) {
            (true, false) => -1,
            (false, true) => 1,
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges() {
        let mut i = Input::default();
        i.advance(button::A);
        assert!(i.pressed(button::A));
        assert!(i.held(button::A));
        i.advance(button::A);
        assert!(!i.pressed(button::A));
        i.advance(0);
        assert!(i.released(button::A));
    }

    #[test]
    fn axes_cancel() {
        let i = Input::new(button::LEFT | button::RIGHT);
        assert_eq!(i.axis_x(), 0);
        let i = Input::new(button::UP);
        assert_eq!(i.axis_y(), -1);
    }
}
