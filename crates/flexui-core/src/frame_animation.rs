//! 帧动画资源定义与单控件播放状态。

use std::sync::Arc;

use flexui_gfx::ImageSource;

/// 帧动画播放方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FramePlayback {
    /// 循环播放。
    #[default]
    Loop,
    /// 播放一遍后停在 `finish` 指定的状态。
    Once,
    /// 初始暂停在第一帧，可通过运行时 API 开始播放。
    Paused,
}

/// 单次动画播放结束后的画面。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameFinish {
    /// 恢复当前状态的静态图片或下层状态动画。
    #[default]
    Restore,
    /// 停在第一帧。
    First,
    /// 停在最后一帧。
    Last,
}

/// 运行时 API 指定动画作用于背景还是前景。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameLayer {
    Background,
    Foreground,
}

/// 不可变、可在多个控件之间共享的帧序列。
#[derive(Debug, Clone, PartialEq)]
pub struct FrameAnimation {
    pub frames: Arc<[ImageSource]>,
    pub fps: f32,
    /// 循环播放时，一轮结束后额外停留在最后一帧的秒数。
    pub loop_interval: f32,
    pub playback: FramePlayback,
    pub finish: FrameFinish,
}

impl FrameAnimation {
    pub fn new(frames: Vec<ImageSource>, fps: f32) -> Self {
        Self {
            frames: frames.into(),
            fps: valid_fps(fps),
            loop_interval: 0.0,
            playback: FramePlayback::Loop,
            finish: FrameFinish::Restore,
        }
    }

    /// 设置循环之间的间隔秒数。负数、NaN 和无穷大按 0 处理。
    pub fn loop_interval(mut self, interval_secs: f32) -> Self {
        self.loop_interval = valid_interval(interval_secs);
        self
    }

    pub fn playback(mut self, playback: FramePlayback) -> Self {
        self.playback = playback;
        self
    }

    pub fn finish(mut self, finish: FrameFinish) -> Self {
        self.finish = finish;
        self
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}

impl Default for FrameAnimation {
    fn default() -> Self {
        Self::new(Vec::new(), 25.0)
    }
}

fn valid_fps(fps: f32) -> f32 {
    if fps.is_finite() && fps > 0.0 {
        fps
    } else {
        25.0
    }
}

fn valid_interval(interval_secs: f32) -> f32 {
    if interval_secs.is_finite() && interval_secs > 0.0 {
        interval_secs
    } else {
        0.0
    }
}

/// 每个控件独立持有的播放进度。资源可共享，进度不可共享。
#[derive(Debug, Clone, Default)]
pub(crate) struct FramePlayer {
    animation: Option<FrameAnimation>,
    elapsed: f32,
    frame: usize,
    finished: bool,
    paused: bool,
}

impl FramePlayer {
    /// 同步当前视觉状态对应的动画；状态动画发生变化时从第一帧重新播放。
    pub(crate) fn tick_state(&mut self, animation: Option<&FrameAnimation>, dt: f32) -> bool {
        match animation {
            Some(animation) if !animation.is_empty() => {
                if self.animation.as_ref() != Some(animation) {
                    self.start(animation.clone());
                    return true;
                }
                self.tick(dt)
            }
            _ => self.stop(),
        }
    }

    /// 显式重新开始一个动画（点击触发及运行时 API 使用）。
    pub(crate) fn start(&mut self, animation: FrameAnimation) {
        self.paused = animation.playback == FramePlayback::Paused;
        self.animation = Some(animation);
        self.elapsed = 0.0;
        self.frame = 0;
        self.finished = false;
    }

    pub(crate) fn pause(&mut self) -> bool {
        if self.animation.is_some() && !self.paused {
            self.paused = true;
            true
        } else {
            false
        }
    }

    pub(crate) fn resume(&mut self) -> bool {
        if self.animation.is_some() && self.paused {
            self.paused = false;
            true
        } else {
            false
        }
    }

    pub(crate) fn stop(&mut self) -> bool {
        let changed = self.image().is_some();
        self.animation = None;
        self.elapsed = 0.0;
        self.frame = 0;
        self.finished = false;
        self.paused = false;
        changed
    }

    /// 推进时间并返回可见帧是否变化。按总时间选帧，卡顿后会自动跳帧。
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        let Some(animation) = self.animation.as_ref() else {
            return false;
        };
        if self.paused || self.finished || animation.frames.is_empty() {
            return false;
        }
        let old = self.visible_frame();
        self.elapsed += dt.max(0.0);
        match animation.playback {
            FramePlayback::Loop | FramePlayback::Paused => {
                let fps = valid_fps(animation.fps);
                let playback_duration = animation.frames.len() as f32 / fps;
                let cycle_duration = playback_duration + valid_interval(animation.loop_interval);
                let cycle_elapsed = self.elapsed % cycle_duration;
                self.frame = if cycle_elapsed >= playback_duration {
                    animation.frames.len() - 1
                } else {
                    ((cycle_elapsed * fps).floor() as usize).min(animation.frames.len() - 1)
                };
            }
            FramePlayback::Once => {
                let absolute = (self.elapsed * valid_fps(animation.fps)).floor() as usize;
                if absolute >= animation.frames.len() {
                    self.finished = true;
                    self.frame = animation.frames.len() - 1;
                } else {
                    self.frame = absolute;
                }
            }
        }
        old != self.visible_frame()
    }

    pub(crate) fn image(&self) -> Option<&ImageSource> {
        let animation = self.animation.as_ref()?;
        let frame = self.visible_frame()?;
        animation.frames.get(frame)
    }

    /// 尚未经过首个 tick 时，绘制端可直接显示定义中的第一帧。
    pub(crate) fn image_for_state<'a>(
        &'a self,
        animation: Option<&'a FrameAnimation>,
    ) -> Option<&'a ImageSource> {
        let animation = animation.filter(|animation| !animation.is_empty())?;
        if self.animation.as_ref() == Some(animation) {
            self.image()
        } else {
            animation.frames.first()
        }
    }

    fn visible_frame(&self) -> Option<usize> {
        let animation = self.animation.as_ref()?;
        if !self.finished {
            return Some(self.frame);
        }
        match animation.finish {
            FrameFinish::Restore => None,
            FrameFinish::First => Some(0),
            FrameFinish::Last => animation.frames.len().checked_sub(1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sequence(playback: FramePlayback, finish: FrameFinish) -> FrameAnimation {
        FrameAnimation::new(
            vec![
                ImageSource::path("1.png"),
                ImageSource::path("2.png"),
                ImageSource::path("3.png"),
            ],
            10.0,
        )
        .playback(playback)
        .finish(finish)
    }

    #[test]
    fn loop_uses_elapsed_time_and_skips_frames() {
        let animation = sequence(FramePlayback::Loop, FrameFinish::Restore);
        let mut player = FramePlayer::default();
        assert!(player.tick_state(Some(&animation), 0.0));
        assert_eq!(player.image(), animation.frames.first());
        assert!(player.tick_state(Some(&animation), 0.21));
        assert_eq!(player.image(), animation.frames.get(2));
        assert!(player.tick_state(Some(&animation), 0.1));
        assert_eq!(player.image(), animation.frames.first());
    }

    #[test]
    fn once_restore_removes_the_overlay_after_last_frame() {
        let animation = sequence(FramePlayback::Once, FrameFinish::Restore);
        let mut player = FramePlayer::default();
        player.start(animation);
        assert!(player.image().is_some());
        assert!(player.tick(0.31));
        assert!(player.image().is_none());
        assert!(!player.tick(1.0));
    }

    #[test]
    fn changing_state_restarts_at_first_frame() {
        let first = sequence(FramePlayback::Loop, FrameFinish::Restore);
        let second = FrameAnimation::new(
            vec![ImageSource::path("a.png"), ImageSource::path("b.png")],
            10.0,
        );
        let mut player = FramePlayer::default();
        player.tick_state(Some(&first), 0.0);
        player.tick_state(Some(&first), 0.11);
        assert_eq!(player.image(), first.frames.get(1));
        assert!(player.tick_state(Some(&second), 0.0));
        assert_eq!(player.image(), second.frames.first());
        assert!(player.tick_state(None, 0.0));
        assert!(player.image().is_none());
    }

    #[test]
    fn paused_animation_advances_after_resume() {
        let animation = sequence(FramePlayback::Paused, FrameFinish::Restore);
        let mut player = FramePlayer::default();
        player.start(animation.clone());
        assert!(!player.tick(0.21));
        assert_eq!(player.image(), animation.frames.first());
        assert!(player.resume());
        assert!(player.tick(0.21));
        assert_eq!(player.image(), animation.frames.get(2));
    }

    #[test]
    fn loop_interval_holds_last_frame_before_restarting() {
        let animation = sequence(FramePlayback::Loop, FrameFinish::Restore).loop_interval(0.2);
        let mut player = FramePlayer::default();
        player.start(animation.clone());

        assert!(player.tick(0.21));
        assert_eq!(player.image(), animation.frames.get(2));
        assert!(!player.tick(0.18));
        assert_eq!(player.image(), animation.frames.get(2));
        assert!(player.tick(0.12));
        assert_eq!(player.image(), animation.frames.first());
    }

    #[test]
    fn once_playback_ignores_loop_interval() {
        let animation = sequence(FramePlayback::Once, FrameFinish::Last).loop_interval(1.0);
        let mut player = FramePlayer::default();
        player.start(animation.clone());
        assert!(player.tick(0.31));
        assert_eq!(player.image(), animation.frames.last());
    }
}
