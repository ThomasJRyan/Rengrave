//! UI control state: the `UiControls` settings struct, the `ToolView` mode
//! selector, and all the user-facing choice enums (units, bit shape, arc fit,
//! justification, origin, etc.) plus their parsing/serialization to and from
//! `LegacySettings`.

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct UiControls {
    pub(crate) cut_type: CutTypeChoice,
    pub(crate) units: UnitsChoice,
    pub(crate) bit_shape: BitShapeChoice,
    pub(crate) arc_fit: ArcFitChoice,
    pub(crate) height_calc: HeightCalcChoice,
    pub(crate) v_check_all: VCheckScopeChoice,
    pub(crate) origin: OriginChoice,
    pub(crate) justify: JustifyChoice,
    pub(crate) yscale: f64,
    pub(crate) xscale_percent: f64,
    pub(crate) line_space: f64,
    pub(crate) char_space_percent: f64,
    pub(crate) word_space_percent: f64,
    pub(crate) angle_degrees: f64,
    pub(crate) text_radius: f64,
    pub(crate) safe_z: f64,
    pub(crate) depth_z: f64,
    pub(crate) stroke_thickness: f64,
    pub(crate) xorigin: f64,
    pub(crate) yorigin: f64,
    pub(crate) segarc: f64,
    pub(crate) accuracy: f64,
    pub(crate) feed: f64,
    pub(crate) plunge: f64,
    pub(crate) boxgap: f64,
    pub(crate) v_bit_angle: f64,
    pub(crate) v_bit_dia: f64,
    pub(crate) v_step_len: f64,
    pub(crate) v_drv_crner: f64,
    pub(crate) v_stp_crner: f64,
    pub(crate) allowance: f64,
    pub(crate) v_max_cut: f64,
    pub(crate) v_rough_stk: f64,
    pub(crate) v_depth_lim: f64,
    pub(crate) clean_dia: f64,
    pub(crate) clean_step: f64,
    pub(crate) clean_v: f64,
    pub(crate) clean_paths: String,
    pub(crate) profile_enabled: bool,
    pub(crate) profile_margin: f64,
    pub(crate) profile_radius: f64,
    pub(crate) profile_depth: f64,
    pub(crate) profile_steps: f64,
    pub(crate) profile_endmill_dia: f64,
    pub(crate) profile_tabs: f64,
    pub(crate) profile_tab_height: f64,
    pub(crate) profile_tab_width: f64,
    pub(crate) profile_chamfer: bool,
    pub(crate) profile_chamfer_depth: f64,
    pub(crate) profile_chamfer_angle: f64,
    pub(crate) profile_width: f64,
    pub(crate) profile_height: f64,
    pub(crate) profile_aspect: f64,
    pub(crate) profile_trace: f64,
    pub(crate) profile_alignment: OriginChoice,
    pub(crate) bmp_turn_policy: BitmapTurnPolicy,
    pub(crate) bmp_turds: f64,
    pub(crate) bmp_alpha: f64,
    pub(crate) bmp_optto: f64,
    pub(crate) bitmap_backend: BitmapBackend,
    pub(crate) gpre: String,
    pub(crate) gpost: String,
    pub(crate) return_to_origin: bool,
    pub(crate) flip: bool,
    pub(crate) mirror: bool,
    pub(crate) outer: bool,
    pub(crate) upper: bool,
    pub(crate) plotbox: bool,
    pub(crate) use_image_size: bool,
    pub(crate) inlay: bool,
    pub(crate) bmp_long: bool,
    pub(crate) recovery_comments: bool,
    pub(crate) var_dis: bool,
    pub(crate) ext_char: bool,
    pub(crate) v_flop: bool,
    pub(crate) v_pplot: bool,
    pub(crate) show_thick: bool,
    pub(crate) show_v_area: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolView {
    TextEngrave,
    ImageEngrave,
    TextVCarve,
    ImageVCarve,
    TextInlay,
    ImageInlay,
}

impl ToolView {
    pub(crate) const ALL: [Self; 6] = [
        Self::TextEngrave,
        Self::TextVCarve,
        Self::TextInlay,
        Self::ImageEngrave,
        Self::ImageVCarve,
        Self::ImageInlay,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::TextEngrave => "Text Engrave",
            Self::ImageEngrave => "Image Engrave",
            Self::TextVCarve => "Text V-carve",
            Self::ImageVCarve => "Image V-carve",
            Self::TextInlay => "Text Inlay",
            Self::ImageInlay => "Image Inlay",
        }
    }

    pub(crate) fn index(self) -> usize {
        match self {
            Self::TextEngrave => 0,
            Self::TextVCarve => 1,
            Self::TextInlay => 2,
            Self::ImageEngrave => 3,
            Self::ImageVCarve => 4,
            Self::ImageInlay => 5,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::TextEngrave => "text-engrave",
            Self::ImageEngrave => "image-engrave",
            Self::TextVCarve => "text-v-carve",
            Self::ImageVCarve => "image-v-carve",
            Self::TextInlay => "text-inlay",
            Self::ImageInlay => "image-inlay",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "text-engrave" => Some(Self::TextEngrave),
            "image-engrave" => Some(Self::ImageEngrave),
            "text-v-carve" => Some(Self::TextVCarve),
            "image-v-carve" => Some(Self::ImageVCarve),
            "text-inlay" => Some(Self::TextInlay),
            "image-inlay" => Some(Self::ImageInlay),
            _ => None,
        }
    }

    pub(crate) fn category_label(self) -> &'static str {
        if self.uses_text() {
            "Text generation"
        } else {
            "Image generation"
        }
    }

    pub(crate) fn uses_text(self) -> bool {
        matches!(self, Self::TextEngrave | Self::TextVCarve | Self::TextInlay)
    }

    pub(crate) fn uses_image(self) -> bool {
        matches!(
            self,
            Self::ImageEngrave | Self::ImageVCarve | Self::ImageInlay
        )
    }

    pub(crate) fn uses_vcarve(self) -> bool {
        matches!(
            self,
            Self::TextVCarve | Self::ImageVCarve | Self::TextInlay | Self::ImageInlay
        )
    }

    pub(crate) fn uses_inlay(self) -> bool {
        matches!(self, Self::TextInlay | Self::ImageInlay)
    }

    pub(crate) fn cut_type(self) -> CutTypeChoice {
        if self.uses_vcarve() {
            CutTypeChoice::VCarve
        } else {
            CutTypeChoice::Engrave
        }
    }

    pub(crate) fn accepts_kind(self, kind: InputCatalogKind) -> bool {
        match kind {
            InputCatalogKind::CxfFont | InputCatalogKind::TtfFont => self.uses_text(),
            InputCatalogKind::Dxf | InputCatalogKind::Svg | InputCatalogKind::Bitmap => {
                self.uses_image()
            }
        }
    }

    pub(crate) fn with_input_kind(self, kind: InputCatalogKind) -> Self {
        let vcarve = self.uses_vcarve();
        let inlay = self.uses_inlay();
        match kind {
            InputCatalogKind::CxfFont | InputCatalogKind::TtfFont => {
                if inlay {
                    Self::TextInlay
                } else if vcarve {
                    Self::TextVCarve
                } else {
                    Self::TextEngrave
                }
            }
            InputCatalogKind::Dxf | InputCatalogKind::Svg | InputCatalogKind::Bitmap => {
                if inlay {
                    Self::ImageInlay
                } else if vcarve {
                    Self::ImageVCarve
                } else {
                    Self::ImageEngrave
                }
            }
        }
    }

    pub(crate) fn with_inlay(self, inlay: bool) -> Self {
        match (
            self.uses_text(),
            self.uses_image(),
            self.uses_vcarve(),
            inlay,
        ) {
            (true, _, true, true) => Self::TextInlay,
            (true, _, true, false) => Self::TextVCarve,
            (_, true, true, true) => Self::ImageInlay,
            (_, true, true, false) => Self::ImageVCarve,
            _ => self,
        }
    }

    pub(crate) fn from_settings_and_path(settings: &LegacySettings, path: Option<&Path>) -> Self {
        let cut_type = CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave"));
        let inlay = get_legacy_bool(settings, "inlay", false);
        let image_input = path
            .and_then(InputCatalogKind::from_path)
            .is_some_and(|kind| {
                matches!(
                    kind,
                    InputCatalogKind::Dxf | InputCatalogKind::Svg | InputCatalogKind::Bitmap
                )
            });
        match (cut_type, image_input, inlay) {
            (CutTypeChoice::VCarve, true, true) => Self::ImageInlay,
            (CutTypeChoice::VCarve, false, true) => Self::TextInlay,
            (CutTypeChoice::VCarve, true, false) => Self::ImageVCarve,
            (CutTypeChoice::VCarve, false, false) => Self::TextVCarve,
            (CutTypeChoice::Engrave, true, _) => Self::ImageEngrave,
            (CutTypeChoice::Engrave, false, _) => Self::TextEngrave,
        }
    }
}

impl UiControls {
    pub(crate) fn from_settings(settings: &LegacySettings) -> Self {
        let explicit_units = settings.get_last("units");
        let source_units = explicit_units
            .map(UnitsChoice::parse)
            .unwrap_or(UnitsChoice::Inch);
        let mut controls = Self {
            cut_type: CutTypeChoice::parse(settings.get_last("cut_type").unwrap_or("engrave")),
            units: source_units,
            bit_shape: BitShapeChoice::parse(settings.get_last("bit_shape").unwrap_or("VBIT")),
            arc_fit: ArcFitChoice::parse(settings.get_last("arc_fit").unwrap_or("none")),
            height_calc: HeightCalcChoice::parse(settings.get_last("H_CALC").unwrap_or("max_use")),
            v_check_all: VCheckScopeChoice::parse(
                settings.get_last("v_check_all").unwrap_or("all"),
            ),
            origin: OriginChoice::parse(settings.get_last("origin").unwrap_or("Default")),
            justify: JustifyChoice::parse(settings.get_last("justify").unwrap_or("Left")),
            yscale: setting_f64(settings, "YSCALE", 2.0),
            xscale_percent: setting_f64(settings, "XSCALE", 100.0),
            line_space: setting_f64(settings, "LSPACE", 1.1),
            char_space_percent: setting_f64(settings, "CSPACE", 25.0),
            word_space_percent: setting_f64(settings, "WSPACE", 100.0),
            angle_degrees: setting_f64(settings, "TANGLE", 0.0),
            text_radius: setting_f64(settings, "TRADIUS", 0.0),
            safe_z: setting_f64(settings, "ZSAFE", 0.25),
            depth_z: setting_f64(settings, "ZCUT", -0.005),
            stroke_thickness: setting_f64(settings, "STHICK", 0.01),
            xorigin: setting_f64(settings, "xorigin", 0.0),
            yorigin: setting_f64(settings, "yorigin", 0.0),
            segarc: setting_f64(settings, "segarc", 5.0),
            accuracy: setting_f64(settings, "accuracy", 0.001),
            feed: setting_f64(settings, "FEED", 5.0),
            plunge: setting_f64(settings, "PLUNGE", 0.0),
            boxgap: setting_f64(settings, "boxgap", 0.25),
            v_bit_angle: setting_f64(settings, "v_bit_angle", 60.0),
            v_bit_dia: setting_f64(settings, "v_bit_dia", 0.5),
            v_step_len: setting_f64(settings, "v_step_len", 0.01),
            v_drv_crner: setting_f64(settings, "v_drv_crner", 135.0),
            v_stp_crner: setting_f64(settings, "v_stp_crner", 200.0),
            allowance: setting_f64(settings, "allowance", 0.0),
            v_max_cut: setting_f64(settings, "v_max_cut", -1.0),
            v_rough_stk: setting_f64(settings, "v_rough_stk", 0.0),
            v_depth_lim: setting_f64(settings, "v_depth_lim", 0.0),
            clean_dia: setting_f64(settings, "clean_dia", 0.25),
            clean_step: setting_f64(settings, "clean_step", 50.0),
            clean_v: setting_f64(settings, "clean_v", 0.05),
            clean_paths: settings
                .get_last("clean_paths")
                .unwrap_or("1,1,0,1,0,1,0,0")
                .to_owned(),
            profile_enabled: get_legacy_bool(settings, "profile_cut", false),
            profile_margin: setting_f64(settings, "profile_margin", 0.25),
            profile_radius: setting_f64(settings, "profile_radius", 0.0),
            profile_depth: setting_f64(settings, "profile_depth", 0.125),
            profile_steps: setting_f64(settings, "profile_steps", 1.0),
            profile_endmill_dia: setting_f64(settings, "profile_endmill_dia", 0.25),
            profile_tabs: setting_f64(settings, "profile_tabs", 0.0),
            profile_tab_height: setting_f64(settings, "profile_tab_height", 1.0 / 25.4),
            profile_tab_width: setting_f64(settings, "profile_tab_width", 0.0),
            profile_chamfer: get_legacy_bool(settings, "profile_chamfer", false),
            profile_chamfer_depth: setting_f64(settings, "profile_chamfer_depth", 0.02),
            profile_chamfer_angle: setting_f64(settings, "profile_chamfer_angle", 60.0),
            profile_width: setting_f64(settings, "profile_width", 0.0),
            profile_height: setting_f64(settings, "profile_height", 0.0),
            profile_aspect: setting_f64(settings, "profile_aspect", 0.0),
            profile_trace: setting_f64(settings, "profile_trace", 0.0),
            profile_alignment: OriginChoice::parse(
                settings.get_last("profile_align").unwrap_or("Mid-Center"),
            ),
            bmp_turn_policy: BitmapTurnPolicy::parse(
                settings.get_last("bmp_turnp").unwrap_or("minority"),
            ),
            bmp_turds: setting_f64(settings, "bmp_turds", 2.0),
            bmp_alpha: setting_f64(settings, "bmp_alpha", 1.0),
            bmp_optto: setting_f64(settings, "bmp_optto", 0.2),
            bitmap_backend: BitmapBackend::from_settings(settings),
            gpre: settings
                .get_last("gpre")
                .unwrap_or(DEFAULT_GCODE_PREAMBLE)
                .to_owned(),
            gpost: settings
                .get_last("gpost")
                .unwrap_or(DEFAULT_GCODE_POSTAMBLE)
                .to_owned(),
            return_to_origin: get_legacy_bool(settings, "return_to_origin", true),
            flip: get_legacy_bool(settings, "flip", false),
            mirror: get_legacy_bool(settings, "mirror", false),
            outer: get_legacy_bool(settings, "outer", true),
            upper: get_legacy_bool(settings, "upper", true),
            plotbox: get_legacy_bool(settings, "plotbox", false),
            use_image_size: get_legacy_bool(settings, "useIMGsize", false),
            inlay: get_legacy_bool(settings, "inlay", false),
            bmp_long: get_legacy_bool(settings, "bmp_long", true),
            recovery_comments: !get_legacy_bool(settings, "no_comments", false),
            var_dis: get_legacy_bool(settings, "var_dis", true),
            ext_char: get_legacy_bool(settings, "ext_char", false),
            v_flop: get_legacy_bool(settings, "v_flop", false),
            v_pplot: get_legacy_bool(settings, "v_pplot", false),
            show_thick: get_legacy_bool(settings, "show_thick", true),
            show_v_area: get_legacy_bool(settings, "show_v_area", true),
        };
        if explicit_units.is_none() {
            controls.convert_units(UnitsChoice::default_ui());
        }
        controls
    }

    pub(crate) fn convert_units(&mut self, target_units: UnitsChoice) {
        if self.units == target_units {
            return;
        }

        let factor = self.units.conversion_factor_to(target_units);
        self.units = target_units;

        self.yscale *= factor;
        self.text_radius *= factor;
        self.safe_z *= factor;
        self.depth_z *= factor;
        self.stroke_thickness *= factor;
        self.xorigin *= factor;
        self.yorigin *= factor;
        self.accuracy *= factor;
        self.feed *= factor;
        self.plunge *= factor;
        self.boxgap *= factor;
        self.v_bit_dia *= factor;
        self.v_step_len *= factor;
        self.allowance *= factor;
        self.v_max_cut *= factor;
        self.v_rough_stk *= factor;
        self.v_depth_lim *= factor;
        self.clean_dia *= factor;
        self.clean_v *= factor;
        self.profile_margin *= factor;
        self.profile_radius *= factor;
        self.profile_depth *= factor;
        self.profile_endmill_dia *= factor;
        self.profile_tab_height *= factor;
        self.profile_tab_width *= factor;
        self.profile_chamfer_depth *= factor;
        self.profile_width *= factor;
        self.profile_height *= factor;
    }

    pub(crate) fn overrides(&self) -> Vec<LegacySetting> {
        let mut entries = Vec::new();
        push_setting(&mut entries, "cut_type", self.cut_type.value(), false);
        push_setting(&mut entries, "units", self.units.value(), false);
        push_setting(&mut entries, "bit_shape", self.bit_shape.value(), false);
        push_setting(&mut entries, "arc_fit", self.arc_fit.value(), false);
        push_setting(&mut entries, "H_CALC", self.height_calc.value(), false);
        push_setting(&mut entries, "v_check_all", self.v_check_all.value(), false);
        push_setting(&mut entries, "origin", self.origin.value(), false);
        push_setting(&mut entries, "justify", self.justify.value(), false);
        push_setting(
            &mut entries,
            "YSCALE",
            format_setting_number(self.yscale),
            false,
        );
        push_setting(
            &mut entries,
            "XSCALE",
            format_setting_number(self.xscale_percent),
            false,
        );
        push_setting(
            &mut entries,
            "LSPACE",
            format_setting_number(self.line_space),
            false,
        );
        push_setting(
            &mut entries,
            "CSPACE",
            format_setting_number(self.char_space_percent),
            false,
        );
        push_setting(
            &mut entries,
            "WSPACE",
            format_setting_number(self.word_space_percent),
            false,
        );
        push_setting(
            &mut entries,
            "TANGLE",
            format_setting_number(self.angle_degrees),
            false,
        );
        push_setting(
            &mut entries,
            "TRADIUS",
            format_setting_number(self.text_radius),
            false,
        );
        push_setting(
            &mut entries,
            "ZSAFE",
            format_setting_number(self.safe_z),
            false,
        );
        push_setting(
            &mut entries,
            "ZCUT",
            format_setting_number(self.depth_z),
            false,
        );
        push_setting(
            &mut entries,
            "STHICK",
            format_setting_number(self.stroke_thickness),
            false,
        );
        push_setting(
            &mut entries,
            "xorigin",
            format_setting_number(self.xorigin),
            false,
        );
        push_setting(
            &mut entries,
            "yorigin",
            format_setting_number(self.yorigin),
            false,
        );
        push_setting(
            &mut entries,
            "segarc",
            format_setting_number(self.segarc),
            false,
        );
        push_setting(
            &mut entries,
            "accuracy",
            format_setting_number(self.accuracy),
            false,
        );
        push_setting(
            &mut entries,
            "FEED",
            format_setting_number(self.feed),
            false,
        );
        push_setting(
            &mut entries,
            "PLUNGE",
            format_setting_number(self.plunge),
            false,
        );
        push_setting(
            &mut entries,
            "boxgap",
            format_setting_number(self.boxgap),
            false,
        );
        push_setting(
            &mut entries,
            "v_bit_angle",
            format_setting_number(self.v_bit_angle),
            false,
        );
        push_setting(
            &mut entries,
            "v_bit_dia",
            format_setting_number(self.v_bit_dia),
            false,
        );
        push_setting(
            &mut entries,
            "v_step_len",
            format_setting_number(self.v_step_len),
            false,
        );
        push_setting(
            &mut entries,
            "v_drv_crner",
            format_setting_number(self.v_drv_crner),
            false,
        );
        push_setting(
            &mut entries,
            "v_stp_crner",
            format_setting_number(self.v_stp_crner),
            false,
        );
        push_setting(
            &mut entries,
            "allowance",
            format_setting_number(self.allowance),
            false,
        );
        push_setting(
            &mut entries,
            "v_max_cut",
            format_setting_number(self.v_max_cut),
            false,
        );
        push_setting(
            &mut entries,
            "v_rough_stk",
            format_setting_number(self.v_rough_stk),
            false,
        );
        push_setting(
            &mut entries,
            "v_depth_lim",
            format_setting_number(self.v_depth_lim),
            false,
        );
        push_setting(
            &mut entries,
            "clean_dia",
            format_setting_number(self.clean_dia),
            false,
        );
        push_setting(
            &mut entries,
            "clean_step",
            format_setting_number(self.clean_step),
            false,
        );
        push_setting(
            &mut entries,
            "clean_v",
            format_setting_number(self.clean_v),
            false,
        );
        push_setting(&mut entries, "clean_paths", self.clean_paths.trim(), false);
        push_bool(&mut entries, "profile_cut", self.profile_enabled);
        push_setting(
            &mut entries,
            "profile_margin",
            format_setting_number(self.profile_margin),
            false,
        );
        push_setting(
            &mut entries,
            "profile_radius",
            format_setting_number(self.profile_radius),
            false,
        );
        push_setting(
            &mut entries,
            "profile_depth",
            format_setting_number(self.profile_depth),
            false,
        );
        push_setting(
            &mut entries,
            "profile_steps",
            format_setting_number(self.profile_steps.round().max(1.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_endmill_dia",
            format_setting_number(self.profile_endmill_dia),
            false,
        );
        push_setting(
            &mut entries,
            "profile_tabs",
            format_setting_number(self.profile_tabs.round().max(0.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_tab_height",
            format_setting_number(self.profile_tab_height),
            false,
        );
        push_setting(
            &mut entries,
            "profile_tab_width",
            format_setting_number(self.profile_tab_width.max(0.0)),
            false,
        );
        push_bool(&mut entries, "profile_chamfer", self.profile_chamfer);
        push_setting(
            &mut entries,
            "profile_chamfer_depth",
            format_setting_number(self.profile_chamfer_depth),
            false,
        );
        push_setting(
            &mut entries,
            "profile_chamfer_angle",
            format_setting_number(self.profile_chamfer_angle),
            false,
        );
        push_setting(
            &mut entries,
            "profile_width",
            format_setting_number(self.profile_width.max(0.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_height",
            format_setting_number(self.profile_height.max(0.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_aspect",
            format_setting_number(self.profile_aspect.max(0.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_trace",
            format_setting_number(self.profile_trace.clamp(0.0, 100.0)),
            false,
        );
        push_setting(
            &mut entries,
            "profile_align",
            self.profile_alignment.value(),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_turnp",
            self.bmp_turn_policy.value(),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_turds",
            format_setting_number(self.bmp_turds),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_alpha",
            format_setting_number(self.bmp_alpha),
            false,
        );
        push_setting(
            &mut entries,
            "bmp_optto",
            format_setting_number(self.bmp_optto),
            false,
        );
        push_setting(
            &mut entries,
            "bitmap_backend",
            self.bitmap_backend.value(),
            false,
        );
        push_setting(&mut entries, "gpre", self.gpre.trim(), false);
        push_setting(&mut entries, "gpost", self.gpost.trim(), false);
        push_bool(&mut entries, "return_to_origin", self.return_to_origin);
        push_bool(&mut entries, "flip", self.flip);
        push_bool(&mut entries, "mirror", self.mirror);
        push_bool(&mut entries, "outer", self.outer);
        push_bool(&mut entries, "upper", self.upper);
        push_bool(&mut entries, "plotbox", self.plotbox);
        push_bool(&mut entries, "useIMGsize", self.use_image_size);
        push_bool(&mut entries, "inlay", self.inlay);
        push_bool(&mut entries, "bmp_long", self.bmp_long);
        push_bool(&mut entries, "no_comments", !self.recovery_comments);
        push_bool(&mut entries, "var_dis", self.var_dis);
        push_bool(&mut entries, "ext_char", self.ext_char);
        push_bool(&mut entries, "v_flop", self.v_flop);
        push_bool(&mut entries, "v_pplot", self.v_pplot);
        push_bool(&mut entries, "show_thick", self.show_thick);
        push_bool(&mut entries, "show_v_area", self.show_v_area);
        entries
    }
}

pub(crate) fn default_ui_controls() -> UiControls {
    let mut controls = UiControls::from_settings(&default_legacy_settings());
    controls.convert_units(UnitsChoice::default_ui());
    controls
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeightCalcChoice {
    MaxUse,
    MaxAll,
}

impl HeightCalcChoice {
    pub(crate) fn parse(value: &str) -> Self {
        if value == "max_all" {
            Self::MaxAll
        } else {
            Self::MaxUse
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::MaxUse => "max_use",
            Self::MaxAll => "max_all",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::MaxUse => "Used chars",
            Self::MaxAll => "All chars",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitmapTurnPolicy {
    Minority,
    Majority,
    Black,
    White,
    Left,
    Right,
    Random,
}

impl BitmapTurnPolicy {
    pub(crate) const ALL: [Self; 7] = [
        Self::Minority,
        Self::Majority,
        Self::Black,
        Self::White,
        Self::Left,
        Self::Right,
        Self::Random,
    ];

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "majority" => Self::Majority,
            "black" => Self::Black,
            "white" => Self::White,
            "left" => Self::Left,
            "right" => Self::Right,
            "random" => Self::Random,
            _ => Self::Minority,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Minority => "minority",
            Self::Majority => "majority",
            Self::Black => "black",
            Self::White => "white",
            Self::Left => "left",
            Self::Right => "right",
            Self::Random => "random",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Minority => "Minority",
            Self::Majority => "Majority",
            Self::Black => "Black",
            Self::White => "White",
            Self::Left => "Left",
            Self::Right => "Right",
            Self::Random => "Random",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VCheckScopeChoice {
    All,
    Character,
}

impl VCheckScopeChoice {
    pub(crate) const ALL: [Self; 2] = [Self::All, Self::Character];

    pub(crate) fn parse(value: &str) -> Self {
        if value == "chr" {
            Self::Character
        } else {
            Self::All
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Character => "chr",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Character => "Character",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CutTypeChoice {
    Engrave,
    VCarve,
}

impl CutTypeChoice {
    pub(crate) fn parse(value: &str) -> Self {
        if value == "v-carve" {
            Self::VCarve
        } else {
            Self::Engrave
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Engrave => "engrave",
            Self::VCarve => "v-carve",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Engrave => "Engrave",
            Self::VCarve => "V-carve",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnitsChoice {
    Inch,
    Mm,
}

impl UnitsChoice {
    pub(crate) fn default_ui() -> Self {
        Self::Mm
    }

    pub(crate) fn parse(value: &str) -> Self {
        if value == "mm" { Self::Mm } else { Self::Inch }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Inch => "in",
            Self::Mm => "mm",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Inch => "Inch",
            Self::Mm => "mm",
        }
    }

    pub(crate) fn conversion_factor_to(self, target: Self) -> f64 {
        match (self, target) {
            (Self::Inch, Self::Mm) => MM_PER_INCH,
            (Self::Mm, Self::Inch) => 1.0 / MM_PER_INCH,
            _ => 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BitShapeChoice {
    VBit,
    Ball,
    Flat,
}

impl BitShapeChoice {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "BALL" => Self::Ball,
            "FLAT" => Self::Flat,
            _ => Self::VBit,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::VBit => "VBIT",
            Self::Ball => "BALL",
            Self::Flat => "FLAT",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::VBit => "V-bit",
            Self::Ball => "Ball",
            Self::Flat => "Flat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArcFitChoice {
    NoFit,
    Center,
    Radius,
}

impl ArcFitChoice {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "radius" => Self::Radius,
            _ => Self::NoFit,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::NoFit => "none",
            Self::Center => "center",
            Self::Radius => "radius",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::NoFit => "None",
            Self::Center => "Center",
            Self::Radius => "Radius",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum JustifyChoice {
    Left,
    Center,
    Right,
}

impl JustifyChoice {
    pub(crate) const ALL: [Self; 3] = [Self::Left, Self::Center, Self::Right];

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Center" => Self::Center,
            "Right" => Self::Right,
            _ => Self::Left,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Left => "Left",
            Self::Center => "Center",
            Self::Right => "Right",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.value()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OriginChoice {
    Default,
    TopLeft,
    TopCenter,
    TopRight,
    MidLeft,
    MidCenter,
    MidRight,
    BotLeft,
    BotCenter,
    BotRight,
    ArcCenter,
}

impl OriginChoice {
    pub(crate) const ALL: [Self; 11] = [
        Self::Default,
        Self::TopLeft,
        Self::TopCenter,
        Self::TopRight,
        Self::MidLeft,
        Self::MidCenter,
        Self::MidRight,
        Self::BotLeft,
        Self::BotCenter,
        Self::BotRight,
        Self::ArcCenter,
    ];

    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "Top-Left" => Self::TopLeft,
            "Top-Center" => Self::TopCenter,
            "Top-Right" => Self::TopRight,
            "Mid-Left" => Self::MidLeft,
            "Mid-Center" => Self::MidCenter,
            "Mid-Right" => Self::MidRight,
            "Bot-Left" => Self::BotLeft,
            "Bot-Center" => Self::BotCenter,
            "Bot-Right" => Self::BotRight,
            "Arc-Center" => Self::ArcCenter,
            _ => Self::Default,
        }
    }

    pub(crate) fn value(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::TopLeft => "Top-Left",
            Self::TopCenter => "Top-Center",
            Self::TopRight => "Top-Right",
            Self::MidLeft => "Mid-Left",
            Self::MidCenter => "Mid-Center",
            Self::MidRight => "Mid-Right",
            Self::BotLeft => "Bot-Left",
            Self::BotCenter => "Bot-Center",
            Self::BotRight => "Bot-Right",
            Self::ArcCenter => "Arc-Center",
        }
    }

    pub(crate) fn label(self) -> &'static str {
        self.value()
    }
}
