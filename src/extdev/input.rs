use std::collections::VecDeque;

use winit::event::KeyEvent;

pub enum KeyType {
    Home,
    Power,
}

pub enum KeyPress {
    Press(KeyType),
    Release(KeyType),
}

#[derive(Default)]
pub struct Input {
    touch: VecDeque<Option<(usize, usize)>>,
    keys: VecDeque<KeyPress>,
}

impl Input {
    pub fn touch_move(&mut self, xy: (usize, usize)) {
        if let Some(last_touch) = self.touch.back() {
            if last_touch.is_none() {
                self.touch.push_back(Some(xy));
            } else if let Some(prev_xy) = last_touch && *prev_xy != xy {
                self.touch.push_back(Some(xy));
            }
        } else {
            self.touch.push_back(Some(xy));
        }
    }

    #[inline]
    pub fn touch_release(&mut self) {
        self.touch.push_back(None);
    }

    #[inline]
    pub fn check_touch(&mut self) -> Option<Option<(usize, usize)>> {
        self.touch.pop_front()
    }

    #[inline]
    pub fn key_press(&mut self, key: KeyType) {
        self.keys.push_back(KeyPress::Press(key));
    }

    #[inline]
    pub fn key_release(&mut self, key: KeyType) {
        self.keys.push_back(KeyPress::Release(key));
    }

    #[inline]
    pub fn check_key(&mut self) -> Option<KeyPress> {
        self.keys.pop_front()
    }
}
