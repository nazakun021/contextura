use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebounceState {
    Scrolling,
    Settling(Instant),
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebounceEvent {
    Triggered,
    MotionDetected,
    None,
}

pub struct DebounceStateMachine {
    pub state: DebounceState,
    pub debounce_duration: Duration,
    pub motion_threshold: f32,
}

impl Default for DebounceStateMachine {
    fn default() -> Self {
        Self {
            state: DebounceState::Idle,
            debounce_duration: Duration::from_millis(300),
            motion_threshold: 0.05,
        }
    }
}

impl DebounceStateMachine {
    pub fn new(debounce_ms: u64, motion_threshold: f32) -> Self {
        Self {
            state: DebounceState::Idle,
            debounce_duration: Duration::from_millis(debounce_ms),
            motion_threshold,
        }
    }

    pub fn update(&mut self, motion_ratio: f32) -> DebounceEvent {
        let has_motion = motion_ratio > self.motion_threshold;

        match self.state {
            DebounceState::Scrolling => {
                if has_motion {
                    // Still scrolling, stay in state
                    DebounceEvent::MotionDetected
                } else {
                    // Stopped scrolling, begin settling
                    self.state = DebounceState::Settling(Instant::now());
                    DebounceEvent::None
                }
            }
            DebounceState::Settling(start_time) => {
                if has_motion {
                    // False stop, back to scrolling
                    self.state = DebounceState::Scrolling;
                    DebounceEvent::MotionDetected
                } else if start_time.elapsed() >= self.debounce_duration {
                    // Fully settled
                    self.state = DebounceState::Idle;
                    DebounceEvent::Triggered
                } else {
                    // Still settling
                    DebounceEvent::None
                }
            }
            DebounceState::Idle => {
                if has_motion {
                    // New motion detected
                    self.state = DebounceState::Scrolling;
                    DebounceEvent::MotionDetected
                } else {
                    // Still idle
                    DebounceEvent::None
                }
            }
        }
    }
}

pub struct MotionDetector {
    pixel_diff_threshold: u8,
    edge_inset_percent: u32,
    prev_thumbnail: Option<Vec<u8>>, // 160x90 grayscale pixels
    width: usize,
    height: usize,
}

impl MotionDetector {
    pub fn new(pixel_diff_threshold: u8, edge_inset_percent: u32) -> Self {
        Self {
            pixel_diff_threshold,
            edge_inset_percent,
            prev_thumbnail: None,
            width: 160,
            height: 90,
        }
    }

    /// Computed bounding box for active area (excluding inset)
    fn get_active_rect(&self) -> (usize, usize, usize, usize) {
        let max_x_inset = (self.width as f32 * (self.edge_inset_percent as f32 / 100.0)) as usize;
        let max_y_inset = (self.height as f32 * (self.edge_inset_percent as f32 / 100.0)) as usize;
        let min_x = max_x_inset;
        let min_y = max_y_inset;
        let max_x = self.width - max_x_inset;
        let max_y = self.height - max_y_inset;
        (min_x, min_y, max_x, max_y)
    }

    /// Compares two thumbnails and returns a binary mask of changed pixels
    pub fn compute_diff_mask(&self, prev: &[u8], curr: &[u8]) -> Vec<bool> {
        let mut mask = vec![false; self.width * self.height];
        let (min_x, min_y, max_x, max_y) = self.get_active_rect();

        for y in min_y..max_y {
            for x in min_x..max_x {
                let idx = y * self.width + x;
                let diff = (prev[idx] as i32 - curr[idx] as i32).abs() as u8;
                mask[idx] = diff > self.pixel_diff_threshold;
            }
        }
        mask
    }

    /// Finds the largest contiguous region using a simple flood fill approach
    pub fn largest_contiguous_region(&self, mask: &[bool]) -> f32 {
        let (min_x, min_y, max_x, max_y) = self.get_active_rect();
        let total_area = ((max_x - min_x) * (max_y - min_y)) as f32;
        if total_area == 0.0 {
            return 0.0;
        }

        let mut visited = vec![false; self.width * self.height];
        let mut max_region_size = 0;

        for y in min_y..max_y {
            for x in min_x..max_x {
                let idx = y * self.width + x;
                if mask[idx] && !visited[idx] {
                    // Flood fill to find size of this region
                    let size = self.flood_fill(x, y, mask, &mut visited, min_x, min_y, max_x, max_y);
                    if size > max_region_size {
                        max_region_size = size;
                    }
                }
            }
        }

        max_region_size as f32 / total_area
    }

    fn flood_fill(
        &self,
        start_x: usize,
        start_y: usize,
        mask: &[bool],
        visited: &mut [bool],
        min_x: usize,
        min_y: usize,
        max_x: usize,
        max_y: usize,
    ) -> usize {
        let mut stack = vec![(start_x, start_y)];
        let mut size = 0;

        while let Some((x, y)) = stack.pop() {
            let idx = y * self.width + x;
            if visited[idx] {
                continue;
            }
            visited[idx] = true;
            size += 1;

            // Check 4-connected neighbors
            if x > min_x && mask[y * self.width + x - 1] && !visited[y * self.width + x - 1] {
                stack.push((x - 1, y));
            }
            if x + 1 < max_x && mask[y * self.width + x + 1] && !visited[y * self.width + x + 1] {
                stack.push((x + 1, y));
            }
            if y > min_y && mask[(y - 1) * self.width + x] && !visited[(y - 1) * self.width + x] {
                stack.push((x, y - 1));
            }
            if y + 1 < max_y && mask[(y + 1) * self.width + x] && !visited[(y + 1) * self.width + x] {
                stack.push((x, y + 1));
            }
        }

        size
    }

    pub fn process_thumbnail(&mut self, current: Vec<u8>) -> f32 {
        let ratio = if let Some(prev) = &self.prev_thumbnail {
            let mask = self.compute_diff_mask(prev, &current);
            self.largest_contiguous_region(&mask)
        } else {
            1.0 // First frame always counts as motion
        };

        self.prev_thumbnail = Some(current);
        ratio
    }
}

// Ensure tests module follows best practices
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_should_trigger_when_motion_stops() {
        let mut state_machine = DebounceStateMachine::new(100, 0.05); // 100ms
        
        // Start scrolling
        assert_eq!(state_machine.update(0.1), DebounceEvent::MotionDetected);
        assert_eq!(state_machine.state, DebounceState::Scrolling);
        
        // Stop scrolling
        assert_eq!(state_machine.update(0.0), DebounceEvent::None);
        assert!(matches!(state_machine.state, DebounceState::Settling(_)));
        
        std::thread::sleep(Duration::from_millis(150));
        
        // Should trigger after duration
        assert_eq!(state_machine.update(0.0), DebounceEvent::Triggered);
        assert_eq!(state_machine.state, DebounceState::Idle);
    }
}
