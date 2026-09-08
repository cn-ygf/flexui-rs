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

/// 视图进入或退出时移动的边缘，语义与 SwiftUI 的 `Edge` 一致。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionEdge {
    Top,
    Bottom,
    Left,
    Right,
}

/// 声明式显隐过渡。
///
/// 当前提供从指定边缘滑入、沿相反方向滑出的移动过渡；`distance` 是逻辑像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    edge: TransitionEdge,
    distance: f32,
    duration: f32,
    easing: Easing,
}

impl Transition {
    /// 创建边缘滑动过渡。
    pub fn slide(edge: TransitionEdge, distance: f32) -> Self {
        Self {
            edge,
            distance: distance.abs(),
            duration: 0.25,
            easing: Easing::EaseInOut,
        }
    }

    /// 设置动画时长（秒）。
    pub fn duration(mut self, seconds: f32) -> Self {
        self.duration = seconds.max(0.001);
        self
    }

    /// 设置缓动曲线。
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub fn edge(self) -> TransitionEdge {
        self.edge
    }

    pub fn distance(self) -> f32 {
        self.distance
    }

    pub fn duration_secs(self) -> f32 {
        self.duration
    }

    pub fn easing_curve(self) -> Easing {
        self.easing
    }

    /// 相对静止位置的进入起点/退出终点。
    pub(crate) fn offset(self) -> (f32, f32) {
        match self.edge {
            TransitionEdge::Top => (0.0, -self.distance),
            TransitionEdge::Bottom => (0.0, self.distance),
            TransitionEdge::Left => (-self.distance, 0.0),
            TransitionEdge::Right => (self.distance, 0.0),
        }
    }

    pub(crate) fn prop(self) -> AnimProp {
        match self.edge {
            TransitionEdge::Top | TransitionEdge::Bottom => AnimProp::TranslateY,
            TransitionEdge::Left | TransitionEdge::Right => AnimProp::TranslateX,
        }
    }
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
    /// Progress/Slider 的归一化值（夹取到 0~1）。
    Value,
    /// 横向滚动位置。
    ScrollX,
    /// 纵向滚动位置。
    ScrollY,
    /// 控件及其子树的水平视觉位移。
    TranslateX,
    /// 控件及其子树的垂直视觉位移。
    TranslateY,
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
    /// 是否由声明式显隐过渡创建。
    pub(crate) visibility_transition: bool,
}

impl Anim {
    /// 当前插值。
    pub fn value_at(&self) -> f32 {
        let t = if self.dur > 0.0 {
            (self.elapsed / self.dur).clamp(0.0, 1.0)
        } else {
            1.0
        };
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
        for e in [
            Easing::Linear,
            Easing::EaseIn,
            Easing::EaseOut,
            Easing::EaseInOut,
        ] {
            assert!(e.apply(0.0).abs() < 1e-6, "{e:?} @0");
            assert!((e.apply(1.0) - 1.0).abs() < 1e-6, "{e:?} @1");
        }
        assert!((Easing::Linear.apply(0.5) - 0.5).abs() < 1e-6);
        assert!((Easing::EaseInOut.apply(0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn slide_transition_按边缘生成位移和动画轴() {
        let transition = Transition::slide(TransitionEdge::Bottom, 120.0)
            .duration(0.2)
            .easing(Easing::EaseOut);
        assert_eq!(transition.offset(), (0.0, 120.0));
        assert_eq!(transition.prop(), AnimProp::TranslateY);
        assert!((transition.duration_secs() - 0.2).abs() < f32::EPSILON);
        assert_eq!(transition.easing_curve(), Easing::EaseOut);
    }
}
