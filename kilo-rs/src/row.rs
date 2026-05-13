pub const TAB_STOP: usize = 8;

pub struct Row {
    pub chars: Vec<u8>,
    pub render: Vec<u8>,
}

impl Row {
    pub fn new(chars: Vec<u8>) -> Self {
        let mut row = Row { chars, render: Vec::new() };
        row.update_render();
        row
    }

    pub fn update_render(&mut self) {
        let mut render = Vec::with_capacity(self.chars.len());
        for &c in &self.chars {
            if c == b'\t' {
                render.push(b' ');
                while (render.len() + 1) % TAB_STOP != 0 {
                    render.push(b' ');
                }
            } else {
                render.push(c);
            }
        }
        self.render = render;
    }

    pub fn insert_char(&mut self, at: usize, c: u8) {
        let at = at.min(self.chars.len());
        self.chars.insert(at, c);
        self.update_render();
    }

    pub fn del_char(&mut self, at: usize) {
        if at >= self.chars.len() {
            return;
        }
        self.chars.remove(at);
        self.update_render();
    }

    pub fn append(&mut self, s: &[u8]) {
        self.chars.extend_from_slice(s);
        self.update_render();
    }

    pub fn cx_to_rx(&self, cx: usize) -> usize {
        let mut rx = 0usize;
        for &c in self.chars.iter().take(cx) {
            if c == b'\t' {
                rx += (TAB_STOP - 1) - (rx % TAB_STOP);
            }
            rx += 1;
        }
        rx
    }
}
