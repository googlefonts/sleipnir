/// A command to draw a path segment, in absolute and relative forms.
pub struct DrawingCommand {
    /// The absolute form of the command, e.g. "M" for Svg move-to.
    pub(crate) abs: &'static str,
    /// The relative form of the command, e.g. "m" for Svg relative move-to.
    pub(crate) rel: &'static str,
}

/// The syntax set to use for drawing commands.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum DrawingCommandType {
    /// Svg path 'd' attribute syntax, e.g. "M10,10 l5,5".
    Svg,
    /// Kotlin PathBuilder syntax, e.g. "moveTo(10f, 10f) lineToRelative(5f, 5f)".
    Kt,
}

impl DrawingCommandType {
    pub(crate) fn move_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "M", rel: "m" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "moveTo",
                rel: "moveToRelative",
            },
        }
    }

    pub(crate) fn line_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "L", rel: "l" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "lineTo",
                rel: "lineToRelative",
            },
        }
    }

    pub(crate) fn horizontal_line_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "H", rel: "h" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "horizontalLineTo",
                rel: "horizontalLineToRelative",
            },
        }
    }

    pub(crate) fn vertical_line_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "V", rel: "v" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "verticalLineTo",
                rel: "verticalLineToRelative",
            },
        }
    }

    pub(crate) fn curve_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "C", rel: "c" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "curveTo",
                rel: "curveToRelative",
            },
        }
    }

    pub(crate) fn smooth_curve_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "S", rel: "s" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "reflectiveCurveTo",
                rel: "reflectiveCurveToRelative",
            },
        }
    }

    pub(crate) fn quad_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "Q", rel: "q" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "quadTo",
                rel: "quadToRelative",
            },
        }
    }

    pub(crate) fn smooth_quad_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "T", rel: "t" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "reflectiveQuadTo",
                rel: "reflectiveQuadToRelative",
            },
        }
    }

    #[allow(dead_code)]
    pub(crate) fn arc_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "A", rel: "a" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "arcTo",
                rel: "arcToRelative",
            },
        }
    }

    pub(crate) fn close_cmd(&self) -> DrawingCommand {
        match self {
            DrawingCommandType::Svg => DrawingCommand { abs: "Z", rel: "z" },
            DrawingCommandType::Kt => DrawingCommand {
                abs: "            close()\n",
                rel: "            close()\n",
            },
        }
    }

    pub(crate) fn collect_coords(&self, coords: impl Iterator<Item = String>) -> String {
        match self {
            DrawingCommandType::Svg => {
                let mut path = String::with_capacity(256);
                for coord in coords {
                    if !path.is_empty() && !coord.starts_with('-') {
                        path.push(' ');
                    }
                    path.push_str(&coord);
                }
                path
            }
            DrawingCommandType::Kt => format!("({})\n", coords.collect::<Vec<_>>().join(", ")),
        }
    }

    pub(crate) fn padding(&self) -> &'static str {
        match self {
            DrawingCommandType::Svg => "",
            DrawingCommandType::Kt => "            ",
        }
    }
}
