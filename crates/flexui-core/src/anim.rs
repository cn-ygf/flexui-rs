//! 动画基础（L3）：缓动函数 + 可动画属性 + 时间轴补间。
//!
//! 引擎状态由 `Dispatcher` 持有并按帧 `tick_anims(dt)` 推进；后端用一个帧定时器驱动。

/// 缓动曲线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl Easing {
    /// 归一化进度 t(0~1) → 缓动后进度。
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t,
            Easing::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Easing::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - 2.0 * (1.0 - t) * (1.0 - t)
                }
            }
        }
    }
}

/// 可动画的数值属性。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnimProp {
    /// `Base.value`（Progress/Slider，夹取到 0~1）。
    Value,
    /// `Base.scroll_y`（ScrollView/ListView，平滑滚动）。
    ScrollY,
}

/// 一条补间动画。
#[derive(Clone, Copy, Debug)]
pub struct Anim {
    pub target: crate::widget::WidgetId,
    pub prop: AnimProp,
    pub from: f32,
    pub to: f32,
    /// 时长（秒）。
    pub dur: f32,
    /// 已经过时间（秒）。
    pub elapsed: f32,
    pub easing: Easing,
}

impl Anim {
    /// 当前插值。
    pub fn value_at(&self) -> f32 {
        let t = if self.dur > 0.0 { (self.elapsed / self.dur).clamp(0.0, 1.0) } else { 1.0 };
        self.from + (self.to - self.from) * self.easing.apply(t)
    }
    /// 是否结束。
    pub fn done(&self) -> bool {
        self.elapsed >= self.dur
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_端点一致() {
        for e in [Easing::Linear, Easing::EaseIn, Easing::EaseOut, Easing::EaseInOut] {
            assert!(e.apply(0.0).abs() < 1e-6, "{e:?} @0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6, "{e:?} @1");
        }
        assert!((Easing::Linear.apply(0.5) - 0.5).abs() < 1e-6);
        assert!((Easing::EaseInOut.apply(0.5) - 0.5).abs() < 1e-6);
    }
}
