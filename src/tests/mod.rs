use std::cell::Cell;
use std::rc::Rc;

use super::*;

struct Tracked(Rc<Cell<u32>>);

impl Drop for Tracked {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}

mod arena;
mod prop_tests;
mod shared_arena;
