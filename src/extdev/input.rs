#[derive(Default)]
pub struct Input {
    touch: Option<(usize, usize)>,
    changed: bool,
}

impl Input {
    pub fn touch_move(&mut self, xy: (usize, usize)) {
        if self.touch.is_none() {
            self.touch = Some(xy);
            self.changed = true;
        } else if let Some(prev_xy) = self.touch && prev_xy != xy {
            self.touch = Some(xy);
            self.changed = true;
        }
    }

    pub fn touch_release(&mut self) {
        if self.touch.is_some() {
            self.touch = None;
            self.changed = true;
        }
    }

    pub fn check_touch(&mut self) -> Option<&Option<(usize, usize)>> {
        if self.changed {
            self.changed = false;
            Some(&self.touch)
        } else {
            None
        }
    }
}
