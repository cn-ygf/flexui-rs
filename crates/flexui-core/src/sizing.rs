//! 尺寸与对齐模型（L3）。对应需求 W5（控件尺寸三态）+ Flex 对齐缺口。

/// 控件某一轴的尺寸模式。
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Sizing {
    /// 固定像素。
    Fixed(f32),
    /// 按内容/子控件计算（默认）。
    #[default]
    Content,
    /// 自动撑开，填满父容器可用空间（主轴上参与剩余空间分配）。
    Fill,
}

impl Sizing {
    /// 取固定值（仅 Fixed 有）。
    pub fn fixed_value(self) -> Option<f32> {
        match self {
            Sizing::Fixed(v) => Some(v),
            _ => None,
        }
    }
    pub fn is_fill(self) -> bool {
        matches!(self, Sizing::Fill)
    }
}

/// 主轴对齐（justify-content）：子控件在主轴上的排布方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Justify {
    /// 从起点排列（默认）。
    #[default]
    Start,
    /// 居中。
    Center,
    /// 靠末端。
    End,
    /// 两端对齐，间隔均分（首尾贴边）。
    SpaceBetween,
    /// 每项两侧均分间隔。
    SpaceAround,
}

/// 交叉轴对齐（align-items）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// 起点对齐。
    Start,
    /// 居中。
    Center,
    /// 末端对齐。
    End,
    /// 拉伸填满交叉轴（默认）。
    #[default]
    Stretch,
}
