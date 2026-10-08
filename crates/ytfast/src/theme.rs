//! YtFast's look: YouTube Music's own colours and sizes (measured from
//! music.youtube.com), the Inter font, Lucide icons, and the drawing
//! helpers every view uses.

use std::sync::Arc;

use egui::{Color32, CornerRadius, FontId, Galley, Rect, Response, Sense, Stroke, Vec2};

/// YouTube Music's colours.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Behind everything (`#030303`).
    pub window: Color32,
    /// The player bar and menus (`#212121`).
    pub panel: Color32,
    /// Buttons, chips, the chosen row: white at 10%.
    pub surface: Color32,
    /// The same, under the pointer: white at 20%.
    pub surface_hover: Color32,
    /// The search box: white at 15%.
    pub field: Color32,
    /// Hairlines between rows: white at 10% (`--ytmusic-divider`).
    pub outline: Color32,
    /// The lines of the frame: the menu's edge and divider, the top bar's
    /// line, the search box's border: white at 15%
    /// (`--ytmusic-guide-divider`).
    pub divider: Color32,
    pub text: Color32,
    /// Second lines: white at 70%.
    pub secondary: Color32,
    /// Quieter text (`#aaaaaa`).
    pub dim: Color32,
    /// What cannot be used: white at 30%.
    pub faint: Color32,
    /// YouTube's red: the progress line.
    pub accent: Color32,
    /// Subscribe's outline.
    pub subscribe: Color32,
    /// A switch that is on.
    pub switch: Color32,
    /// Menus (`#282828`).
    pub menu: Color32,
    /// Errors (`--ytmusic-color-lightred`).
    pub danger: Color32,
    /// Quiet greys: `#909090` (the account circle, a switch that is off),
    /// `#717171` (what cannot be used), `#606060` (a scroll bar's thumb).
    pub quiet: Color32,
    pub disabled: Color32,
    pub thumb: Color32,
    /// The search box's words before anything is typed, and its icons:
    /// white at 50%.
    pub hint: Color32,
    /// Words on grey buttons, and the white of white buttons (`#f1f1f1`).
    pub button: Color32,
}

/// White at `alpha` (0 to 255), premultiplied as egui wants.
const fn white(alpha: u8) -> Color32 {
    Color32::from_rgba_premultiplied(alpha, alpha, alpha, alpha)
}

pub const PALETTE: Palette = Palette {
    window: Color32::from_rgb(0x03, 0x03, 0x03),
    panel: Color32::from_rgb(0x21, 0x21, 0x21),
    surface: white(26),
    surface_hover: white(51),
    field: white(38),
    outline: white(26),
    divider: white(38),
    text: Color32::from_rgb(0xff, 0xff, 0xff),
    secondary: white(179),
    dim: Color32::from_rgb(0xaa, 0xaa, 0xaa),
    faint: white(77),
    accent: Color32::from_rgb(0xff, 0x00, 0x00),
    subscribe: Color32::from_rgb(0xff, 0x55, 0x77),
    switch: Color32::from_rgb(0x3e, 0xa6, 0xff),
    menu: Color32::from_rgb(0x28, 0x28, 0x28),
    danger: Color32::from_rgb(0xff, 0x4e, 0x45),
    quiet: Color32::from_rgb(0x90, 0x90, 0x90),
    disabled: Color32::from_rgb(0x71, 0x71, 0x71),
    thumb: Color32::from_rgb(0x60, 0x60, 0x60),
    hint: white(128),
    button: Color32::from_rgb(0xf1, 0xf1, 0xf1),
};

pub const TOP_BAR_HEIGHT: f32 = 64.0;
pub const PLAYER_BAR_HEIGHT: f32 = 72.0;
/// The menu on the left, open and closed.
pub const GUIDE_WIDTH: f32 = 240.0;
pub const GUIDE_MINI_WIDTH: f32 = 72.0;

/// The room YouTube Music leaves for a browser's scroll bar
/// (`--ytmusic-scrollbar-width`). YTFast has no such bar, but keeps the
/// room, so that every width is YouTube Music's at the same window width;
/// the page's own scroll bar floats in it.
const SCROLL_BAR: f32 = 12.0;

/// Where a page's content stands, as YouTube Music places it
/// (`--ytmusic-content-width`, centred): 828 wide from 100 in at 1280 with
/// the menu open, 996 with it closed (measured).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    /// The window's width, which picks YouTube Music's breakpoints.
    pub window: f32,
    /// From the page area's left edge to the content's.
    pub left: f32,
    /// The content's width.
    pub width: f32,
}

impl Grid {
    /// For a window `window` wide whose page area (the window less the
    /// menu) is `area` wide.
    pub fn new(window: f32, area: f32) -> Self {
        let width = if window >= 1150.0 {
            (area - 200.0 - SCROLL_BAR).min(1478.0)
        } else if window >= 616.0 {
            area - 112.0 - SCROLL_BAR
        } else {
            window - 32.0
        };
        Self::centred(window, area, width)
    }

    /// Search's results: a narrower column (`--ytmusic-search-width`), 860
    /// wide from 1150, 720 from 936, 560 below.
    pub fn search(window: f32, area: f32) -> Self {
        let most: f32 = if window >= 1150.0 {
            860.0
        } else if window >= 936.0 {
            720.0
        } else {
            560.0
        };
        let width = if window >= 616.0 {
            most.min(area - 112.0 - SCROLL_BAR)
        } else {
            window - 32.0
        };
        Self::centred(window, area, width)
    }

    fn centred(window: f32, area: f32, width: f32) -> Self {
        let width = width.max(0.0);
        Self {
            window,
            left: ((area - SCROLL_BAR - width) / 2.0).max(0.0),
            width,
        }
    }

    /// A card's side and the space between cards, in a shelf `width` wide
    /// (the content, or an album's column), as YouTube Music's `carousel`
    /// rules size them (measured from 960 to 1920 wide): from 1150 wide a
    /// whole number of cards fill the shelf, 5 (6 from 1364) with 24
    /// between; narrower, cards are 180 with 16 between, and the shelf is
    /// cut at its edge. Beside an album's or playlist's header the cards
    /// are small, 160 (`COLLECTION_STYLE_ITEM_SIZE_SMALL`), 16 apart (24
    /// from 1364), and the shelf is cut at its edge (measured at 1280).
    pub fn cards(&self, width: f32, beside_header: bool) -> (f32, f32) {
        if beside_header {
            return (160.0, if self.window >= 1364.0 { 24.0 } else { 16.0 });
        }
        let across: f32 = if self.window >= 1364.0 {
            6.0
        } else if self.window >= 1150.0 {
            5.0
        } else if self.window >= 616.0 {
            return (180.0, 16.0);
        } else {
            return (160.0, 16.0);
        };
        ((width - (across - 1.0) * 24.0) / across, 24.0)
    }

    /// The width of a column of songs in a shelf that scrolls sideways
    /// (Quick picks, Trending), with its padding of 8 each side: 436 under
    /// 1364 (measured at 1280), else a third of the shelf.
    pub fn song_column(&self, width: f32) -> f32 {
        if self.window >= 1364.0 {
            (width - 96.0) / 3.0 + 16.0
        } else if self.window >= 616.0 {
            436.0
        } else {
            (width - 12.0).min(420.0) + 16.0
        }
    }

    /// The space between those columns.
    pub fn song_column_gap(&self) -> f32 {
        if self.window >= 1150.0 { 24.0 } else { 16.0 }
    }

    /// The space above a shelf's title (`--ytmusic-header-padding`).
    pub fn above_title(&self) -> f32 {
        if self.window >= 1150.0 { 32.0 } else { 16.0 }
    }

    /// The space after each shelf but the last
    /// (`--ytmusic-divider-height`).
    pub fn between_shelves(&self) -> f32 {
        if self.window >= 616.0 { 24.0 } else { 16.0 }
    }
}

/// The space under a page's last row (`--ytmusic-base-page-padding-bottom`).
pub const PAGE_FOOT: f32 = 112.0;

/// YouTube Music's large titles (`display-1`: shelves, pages, an album's
/// or an artist's name) in a window `window` wide: 24 under 1150, 28 to
/// 1363 (measured at 1280), 34 to 1577, 45 from 1578.
pub fn display1(window: f32) -> f32 {
    if window >= 1578.0 {
        45.0
    } else if window >= 1364.0 {
        34.0
    } else if window >= 1150.0 {
        28.0
    } else {
        24.0
    }
}

/// Its smaller titles (`display-2`: lists such as Top songs, the top
/// result's name, dialogs): 20 under 1150, 24 from.
pub fn display2(window: f32) -> f32 {
    if window >= 1150.0 { 24.0 } else { 20.0 }
}

fastframe_icons::icons! {
    /// Every icon the window draws: Google's Material Symbols (Outlined,
    /// weight 400; solid where YouTube Music's are, such as play and
    /// pause), the family YouTube Music's own icons come from.
    pub enum Icon {
        prefix: "ytfast-icon-",
        directory: "../assets/icons/",
        Menu => "menu",
        /// The menu's three, as YouTube Music draws them whether their page
        /// is open or not: a solid house, a ring compass, a bookmark.
        Home => "home-fill",
        Explore => "explore",
        Library => "bookmark",
        Search => "search",
        Close => "close",
        /// The top bar's back and forward.
        NavBack => "arrow-back",
        NavForward => "arrow-forward",
        /// A shelf's arrows, and the top result's.
        Back => "chevron-left",
        Forward => "chevron-right",
        Play => "play",
        Pause => "pause",
        SkipBack => "skip-previous",
        SkipForward => "skip-next",
        Shuffle => "shuffle",
        Repeat => "repeat",
        RepeatOne => "repeat-one",
        Volume => "volume",
        Muted => "volume-off",
        ThumbsUp => "thumb-up",
        ThumbsUpFilled => "thumb-up-fill",
        ThumbsDown => "thumb-down",
        ThumbsDownFilled => "thumb-down-fill",
        MoreVertical => "more-vert",
        Plus => "add",
        Check => "check",
        Pin => "pin",
        /// A cover still on its way.
        Music => "music-note",
        /// Lucide's, until YouTube Music's spinner is copied.
        Loading => "loader-circle",
        // Menus' icons, each standing for the icon YouTube names for that
        // entry (MIX, QUEUE_PLAY_NEXT, ADD_TO_REMOTE_QUEUE...).
        Mix => "mix",
        PlayNext => "queue-play-next",
        AddToQueue => "add-to-queue",
        SaveToPlaylist => "playlist-add",
        Album => "album",
        Artist => "artist",
        Share => "share",
        SavedToLibrary => "bookmark-fill",
        RemoveFromQueue => "remove-from-queue",
        RemoveFromPlaylist => "playlist-remove",
        MoveUp => "arrow-upward",
        MoveDown => "arrow-downward",
        Edit => "edit",
        Delete => "delete",
        // Explore's three buttons (MUSIC_NEW_RELEASE, TRENDING_UP,
        // STICKER_EMOTICON).
        NewReleases => "new-releases",
        Charts => "trending-up",
        Moods => "mood",
        History => "history",
        Settings => "settings",
        LogOut => "logout",
        // The account menu's (ACCOUNT_BOX, PAID, PRIVACY_TIP, HELP).
        AccountBox => "account-box",
        Paid => "paid",
        Policy => "policy",
        Help => "help",
        // A playlist's privacy (PUBLIC, LINK, LOCK), and a menu's arrow.
        Public => "public",
        Link => "link",
        Lock => "lock",
        DropDown => "arrow-drop-down",
        /// YouTube's "E" for explicit (MUSIC_EXPLICIT_BADGE).
        Explicit => "explicit",
        /// A sort button's chevron.
        ExpandMore => "expand-more",
    }
}

/// Sets up fonts, icons and egui's colours. Call once, at start. The fonts
/// for other scripts come later, when needed (see [`ScriptFonts`]).
pub fn install(ctx: &egui::Context) -> ScriptFonts {
    let rendering = fastframe_text::detect();
    ctx.set_fonts(font_definitions(rendering, false));
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);

    let p = PALETTE;
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = p.window;
        v.window_fill = p.menu;
        v.window_stroke = Stroke::NONE;
        v.menu_corner_radius = CornerRadius::same(8);
        v.extreme_bg_color = p.surface;
        v.faint_bg_color = p.panel;
        v.override_text_color = Some(p.text);
        v.hyperlink_color = p.text;
        // A browser's: a thin white caret, and selected words on YouTube's
        // blue.
        v.selection.bg_fill = p.switch.gamma_multiply(0.4);
        v.selection.stroke = Stroke::new(1.0, p.text);
        v.text_cursor.stroke = Stroke::new(1.0, p.text);
        v.widgets.noninteractive.bg_fill = p.panel;
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.outline);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.secondary);
        v.widgets.inactive.bg_fill = p.surface;
        v.widgets.inactive.weak_bg_fill = p.surface;
        v.widgets.inactive.bg_stroke = Stroke::NONE;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.hovered.bg_fill = p.surface_hover;
        v.widgets.hovered.weak_bg_fill = p.surface_hover;
        v.widgets.hovered.bg_stroke = Stroke::NONE;
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, p.text);
        v.widgets.active.bg_fill = p.surface_hover;
        v.widgets.active.weak_bg_fill = p.surface_hover;
        v.widgets.active.fg_stroke = Stroke::new(1.0, p.text);
        v.slider_trailing_fill = true;
        v.handle_shape = egui::style::HandleShape::Circle;
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.slider_rail_height = 4.0;
        style.spacing.scroll.bar_width = 8.0;
        style.spacing.scroll.floating = true;
    });
    // fastframe-text's rendering settings, after the visuals.
    ctx.all_styles_mut(|style| rendering.apply_to_visuals(&mut style.visuals));
    ScriptFonts {
        ctx: ctx.clone(),
        rendering,
        state: Loading::NotWanted,
        unsure: std::collections::BTreeSet::new(),
    }
}

/// Inter at every weight and egui's own fonts (emoji among them), and with
/// `scripts` the computer's fonts for the scripts Inter does not draw.
fn font_definitions(
    rendering: fastframe_text::TextRendering,
    scripts: bool,
) -> egui::FontDefinitions {
    let mut fonts = fastframe_fonts::FontSetup::default()
        .system_fallbacks(scripts)
        .definitions();
    roboto(&mut fonts);
    rendering.apply_to(&mut fonts);
    fonts
}

/// YouTube Music's font, Roboto, first at each weight; Inter (and the
/// computer's fonts for other scripts) stay behind it for any letter
/// Roboto lacks. YouTube Music's big titles are in YouTube Sans, which
/// may not be shipped: Roboto Bold stands in for it, as it does on
/// YouTube Music itself (its next choice in every such rule).
fn roboto(fonts: &mut egui::FontDefinitions) {
    use fastframe_fonts::Weight;
    for (name, bytes, weight) in [
        (
            "roboto",
            include_bytes!("../assets/fonts/Roboto-Regular.ttf").as_slice(),
            Weight::Regular,
        ),
        (
            "roboto-medium",
            include_bytes!("../assets/fonts/Roboto-Medium.ttf").as_slice(),
            Weight::Medium,
        ),
        (
            "roboto-bold",
            include_bytes!("../assets/fonts/Roboto-Bold.ttf").as_slice(),
            Weight::Bold,
        ),
    ] {
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
        fonts
            .families
            .entry(weight.family())
            .or_default()
            .insert(0, name.to_owned());
    }
}

/// The fonts for the scripts Inter does not draw (Chinese, Japanese,
/// Korean, Arabic, the Indian scripts...) are the computer's own, read
/// whole into memory: about 60 MB on Windows. They are added only once some
/// words need them, so a library in Latin letters never pays for them.
pub struct ScriptFonts {
    ctx: egui::Context,
    rendering: fastframe_text::TextRendering,
    state: Loading,
    /// Characters seen that Inter and egui's own fonts may not draw, to
    /// look up in them (see [`ScriptFonts::check`]).
    unsure: std::collections::BTreeSet<char>,
}

enum Loading {
    NotWanted,
    /// Being read, on a thread of their own (a fraction of a second).
    Reading(std::sync::mpsc::Receiver<egui::FontDefinitions>),
    Added,
}

/// The most characters kept to look up; past it, the fonts are read.
const MAX_UNSURE: usize = 256;

impl ScriptFonts {
    /// Notes the characters in `text` the fonts in use may not draw.
    pub fn want_for(&mut self, text: &str) {
        if !matches!(self.state, Loading::NotWanted) {
            return;
        }
        self.unsure
            .extend(text.chars().filter(|c| may_need_script_fonts(*c)));
    }

    /// Looks the characters noted up in the fonts in use, and starts
    /// reading the fonts for other scripts when one is missing. Call while
    /// drawing (egui's fonts exist from the first frame).
    pub fn check(&mut self) {
        if self.unsure.is_empty() || !matches!(self.state, Loading::NotWanted) {
            return;
        }
        let unsure = std::mem::take(&mut self.unsure);
        let font = FontId::proportional(14.0);
        let missing = unsure.len() > MAX_UNSURE
            || self
                .ctx
                .fonts_mut(|fonts| unsure.iter().any(|c| !fonts.has_glyph(&font, *c)));
        if missing {
            self.read();
        }
    }

    /// Reads the fonts for other scripts, on a thread of their own.
    fn read(&mut self) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let (ctx, rendering) = (self.ctx.clone(), self.rendering);
        let reading = std::thread::Builder::new()
            .name("ytfast-fonts".into())
            .spawn(move || {
                let _ = sender.send(font_definitions(rendering, true));
                ctx.request_repaint();
            });
        self.state = match reading {
            Ok(_) => Loading::Reading(receiver),
            Err(e) => {
                log::warn!("the fonts for other scripts could not be read: {e}");
                Loading::Added
            }
        };
    }

    /// Hands the fonts to egui once read; they show from the next frame.
    pub fn add_when_read(&mut self) {
        let Loading::Reading(receiver) = &self.state else {
            return;
        };
        match receiver.try_recv() {
            Ok(fonts) => {
                self.ctx.set_fonts(fonts);
                self.ctx.request_repaint();
                self.state = Loading::Added;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                log::warn!("reading the fonts for other scripts stopped part way");
                self.state = Loading::Added;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
        }
    }
}

/// Whether the fonts in use may not draw `c`: anything past Inter's Latin,
/// Greek and Cyrillic, and past the punctuation, symbols and emoji that
/// Inter and egui's own fonts carry. Those are then looked up in the fonts
/// themselves ([`ScriptFonts::check`]).
fn may_need_script_fonts(c: char) -> bool {
    let c = u32::from(c);
    c >= 0x0530
        && !matches!(
            c,
            // Phonetic extensions, then Latin and Greek extended.
            0x1D00..=0x1DBF
                | 0x1E00..=0x1FFF
                // Punctuation, super- and subscripts, currencies.
                | 0x2000..=0x20CF
                // Letterlike symbols, number forms, arrows.
                | 0x2100..=0x21FF
                // Variation selectors, and the byte order mark.
                | 0xFE00..=0xFE0F
                | 0xFEFF
                // Emoji, in egui's own emoji font, and the tags in flag
                // emoji.
                | 0x1F000..=0x1FAFF
                | 0xE0000..=0xE007F
        )
}

pub fn regular(size: f32) -> FontId {
    fastframe_fonts::Weight::Regular.font_id(size)
}

pub fn medium(size: f32) -> FontId {
    fastframe_fonts::Weight::Medium.font_id(size)
}

pub fn bold(size: f32) -> FontId {
    fastframe_fonts::Weight::Bold.font_id(size)
}

/// A menu's width (`tp-yt-paper-listbox.ytmusic-menu-popup-renderer`).
pub const MENU_WIDTH: f32 = 240.0;

/// A menu's 1 border: white@0.10 as the page draws it, over the menu's own
/// `#212121`, so solid here (a see-through edge would let what lies under
/// the menu show through its outermost pixel).
pub const MENU_EDGE: Color32 = Color32::from_rgb(0x37, 0x37, 0x37);

/// YouTube Music's menu frame: `#212121`, a 1 point white@0.10 border,
/// corners 2, 16 above and below the entries, and no shadow.
pub fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PALETTE.panel)
        .stroke(Stroke::new(1.0, MENU_EDGE))
        .corner_radius(CornerRadius::same(2))
        .inner_margin(egui::Margin::symmetric(0, 16))
}

/// The menu a ⋮ button opens, under it.
pub fn menu_popup(button: &Response) -> egui::Popup<'_> {
    egui::Popup::menu(button)
        .frame(menu_frame())
        .width(MENU_WIDTH)
        .gap(0.0)
}

/// The same menu, opened by a right-click, where the pointer is.
pub fn context_menu(response: &Response) -> egui::Popup<'_> {
    egui::Popup::context_menu(response)
        .frame(menu_frame())
        .width(MENU_WIDTH)
}

/// Sets a menu's width and spacing. Call it first inside a menu.
pub fn menu(ui: &mut egui::Ui) {
    ui.set_width(MENU_WIDTH);
    ui.spacing_mut().item_spacing.y = 0.0;
}

/// One entry of a menu, as YouTube Music's: 48 high, its icon 18 at 16
/// in, its words regular 14 at 50 in, white@0.05 under the pointer (or
/// the keyboard). Named `text` for screen readers.
pub fn menu_item(ui: &mut egui::Ui, icon: Icon, text: &str) -> Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 48.0), Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, text));
    if ui.is_rect_visible(rect) {
        if enabled && (response.hovered() || response.has_focus()) {
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, Color32::from_white_alpha(13));
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        // A disabled entry's words are #717171 and its icon #606060.
        let (icon_color, text_color) = if enabled {
            (PALETTE.text, PALETTE.text)
        } else {
            (PALETTE.thumb, PALETTE.disabled)
        };
        let spot = Rect::from_min_size(
            egui::pos2(rect.left() + 16.0, rect.center().y - 9.0),
            Vec2::splat(18.0),
        );
        paint_icon(ui, icon, spot, 18.0, icon_color);
        let words = fit(
            ui,
            text,
            regular(14.0),
            text_color,
            rect.width() - 50.0 - 16.0,
            1,
        );
        let at = egui::pos2(rect.left() + 50.0, rect.center().y - words.size().y / 2.0);
        ui.painter().galley(at, words, text_color);
    }
    response
}

/// One line of text that is cut short with "…" when it does not fit.
pub fn label(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) -> Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).font(font).color(color))
            .truncate()
            .selectable(false),
    )
}

/// Text laid out to fit `width`: on at most `rows` lines, the last one cut
/// short with "…".
pub fn fit(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    ui.fonts_mut(|f| f.layout_job(job(text, font, color, width, rows)))
}

/// [`fit`], with each line centred. Paint it at the middle of where it
/// goes, not at its left.
pub fn fit_centered(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
) -> Arc<Galley> {
    let mut job = job(text, font, color, width, rows);
    job.halign = egui::Align::Center;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// [`fit`], its first line starting `indent` in (room for a badge).
pub fn fit_indented(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
    indent: f32,
) -> Arc<Galley> {
    let mut job = job(text, font, color, width, rows);
    job.sections[0].leading_space = indent;
    ui.fonts_mut(|f| f.layout_job(job))
}

/// [`fit`], with lines `line_height` apart (the description's 19.6).
pub fn fit_lines(
    ui: &egui::Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    rows: usize,
    line_height: f32,
) -> Arc<Galley> {
    let mut job = job(text, font, color, width, rows);
    job.sections[0].format.line_height = Some(line_height);
    ui.fonts_mut(|f| f.layout_job(job))
}

fn job(text: &str, font: FontId, color: Color32, width: f32, rows: usize) -> egui::text::LayoutJob {
    let mut format = egui::TextFormat::simple(font.clone(), color);
    // Lines 1.2 times the size apart, as YouTube Music sets its titles and
    // lists (a card's two-line title is 33.6 high at 14).
    format.line_height = Some(font.size * 1.2);
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), format);
    job.wrap = egui::text::TextWrapping {
        max_width: width.max(0.0),
        max_rows: rows.max(1),
        // One line is cut right at its end, mid-word, as YouTube Music's
        // ellipsis is; longer text wraps at words, as its line clamp does.
        break_anywhere: rows <= 1,
        overflow_character: Some('…'),
    };
    job
}

/// Paints one line of text at `pos` (its left top), cut short with "…" at
/// `width`. Returns the size drawn.
pub fn paint_line(
    ui: &egui::Ui,
    pos: egui::Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> Vec2 {
    let galley = fit(ui, text, font, color, width, 1);
    let size = galley.size();
    ui.painter().galley(pos, galley, color);
    size
}

/// How a round icon button is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// Only the icon; a soft disc under the pointer.
    Plain,
    /// On a disc of white at 10%, as YouTube Music's small buttons are.
    Tonal,
    /// A white disc with a black icon: the main Play button.
    Filled,
}

/// A round button `diameter` across, with `icon` drawn `icon_size` large.
pub fn round_button(
    ui: &mut egui::Ui,
    icon: Icon,
    diameter: f32,
    icon_size: f32,
    style: Round,
    color: Color32,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && ui.is_enabled();
        let pressed = response.is_pointer_button_down_on();
        let (fill, tint) = match style {
            Round::Plain if hovered => (Some(PALETTE.surface), color),
            Round::Plain => (None, color),
            Round::Tonal if hovered => (Some(PALETTE.surface_hover), color),
            Round::Tonal => (Some(PALETTE.surface), color),
            // A main button that cannot be used: no disc, its icon grey.
            Round::Filled if !ui.is_enabled() => (None, PALETTE.thumb),
            Round::Filled => (Some(PALETTE.text), Color32::BLACK),
        };
        let scale = if pressed && style == Round::Filled {
            0.95
        } else {
            1.0
        };
        if let Some(fill) = fill {
            ui.painter()
                .circle_filled(rect.center(), diameter / 2.0 * scale, fill);
        }
        let tint = if ui.is_enabled() || style == Round::Filled {
            tint
        } else {
            tint.gamma_multiply(0.4)
        };
        paint_icon(ui, icon, rect, icon_size * scale, tint);
    }
    if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    }
}

/// A filter button ("Songs", "Albums", a mood): rounded, white at 10%;
/// white with `#030303` words when chosen (the same weight).
pub fn chip(ui: &mut egui::Ui, text: &str, chosen: bool, height: f32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), regular(14.0), PALETTE.text);
    let size = egui::vec2(galley.size().x + 24.0, height);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, text)
    });
    if ui.is_rect_visible(rect) {
        let (fill, color) = if chosen {
            (PALETTE.text, PALETTE.window)
        } else if response.hovered() {
            (PALETTE.surface_hover, PALETTE.text)
        } else {
            (PALETTE.surface, PALETTE.text)
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        let at = rect.center() - galley.size() / 2.0;
        ui.painter()
            .galley_with_override_text_color(at, galley, color);
    }
    response
}

/// YtFast's mark (the app icon) in `rect`.
pub fn paint_logo(ui: &egui::Ui, rect: Rect) {
    egui::Image::new(egui::include_image!("../assets/logo.svg")).paint_at(ui, rect);
}

pub fn paint_icon(ui: &egui::Ui, icon: Icon, rect: Rect, size: f32, tint: Color32) {
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    icon.image(tint, size).paint_at(ui, icon_rect);
}

/// How a pill button is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pill {
    /// White, with black words: the main thing to do.
    Filled,
    /// White at 10%.
    Tonal,
    /// Words in this colour, a white@0.20 ring.
    Outline(Color32),
    /// Words and ring in this colour (Subscribe).
    Ringed(Color32),
    /// Only words, `#f1f1f1`; white@0.10 under the pointer (a dialog's
    /// Cancel).
    Plain,
    /// Only words, blue (a confirmation's buttons).
    Link,
}

/// A pill button 36 high, with an icon before its words when given one.
pub fn pill(ui: &mut egui::Ui, icon: Option<Icon>, text: &str, style: Pill) -> Response {
    pill_sized(ui, icon, text, style, None)
}

/// [`pill`], `width` wide when given (its words and icon centred), else
/// as wide as they are.
pub fn pill_sized(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    text: &str,
    style: Pill,
    width: Option<f32>,
) -> Response {
    let enabled = ui.is_enabled();
    let color = match style {
        _ if !enabled => PALETTE.disabled,
        Pill::Filled => Color32::from_rgb(0x0f, 0x0f, 0x0f),
        Pill::Tonal | Pill::Plain => PALETTE.button,
        Pill::Outline(color) | Pill::Ringed(color) => color,
        Pill::Link => PALETTE.switch,
    };
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), medium(14.0), color);
    // YouTube's icon is 24 with 6 after it, pulled 6 into the padding, so
    // it adds 24 to the width.
    let icon_width = if icon.is_some() { 24.0 } else { 0.0 };
    let width = width.unwrap_or(galley.size().x + icon_width + 32.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.0), Sense::click());
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), text));
    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let radius = CornerRadius::same(18);
        match style {
            // A main button that cannot be pressed yet is grey.
            Pill::Filled if !enabled => {
                ui.painter().rect_filled(rect, radius, PALETTE.surface);
            }
            Pill::Filled => {
                let fill = if hovered {
                    Color32::from_rgb(0xd9, 0xd9, 0xd9)
                } else {
                    Color32::from_rgb(0xf1, 0xf1, 0xf1)
                };
                ui.painter().rect_filled(rect, radius, fill);
            }
            Pill::Tonal => {
                let fill = if hovered {
                    PALETTE.surface_hover
                } else {
                    PALETTE.surface
                };
                ui.painter().rect_filled(rect, radius, fill);
            }
            // YouTube's outline button: a white@0.20 ring; under the
            // pointer filled white@0.20 instead.
            Pill::Outline(_) => {
                if hovered {
                    ui.painter()
                        .rect_filled(rect, radius, PALETTE.surface_hover);
                } else {
                    ui.painter().rect_stroke(
                        rect.shrink(0.5),
                        radius,
                        Stroke::new(1.0, PALETTE.surface_hover),
                        egui::StrokeKind::Inside,
                    );
                }
            }
            Pill::Plain => {
                if hovered && enabled {
                    ui.painter().rect_filled(rect, radius, PALETTE.surface);
                }
            }
            Pill::Link => {
                if hovered && enabled {
                    ui.painter()
                        .rect_filled(rect, radius, PALETTE.switch.gamma_multiply(0.1));
                }
            }
            Pill::Ringed(color) => {
                if hovered {
                    ui.painter().rect_filled(rect, radius, PALETTE.surface);
                }
                ui.painter().rect_stroke(
                    rect.shrink(0.5),
                    radius,
                    Stroke::new(1.0, color),
                    egui::StrokeKind::Inside,
                );
            }
        }
        let content = galley.size().x + icon_width;
        let mut x = rect.center().x - content / 2.0;
        if let Some(icon) = icon {
            let at = Rect::from_min_size(
                egui::pos2(x - 6.0, rect.center().y - 12.0),
                Vec2::splat(24.0),
            );
            paint_icon(ui, icon, at, 24.0, color);
            x += icon_width;
        }
        let at = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
        ui.painter().galley(at, galley, color);
    }
    response
}

/// YouTube Music's switch (`tp-yt-paper-toggle-button`): a bar 36×14, r 7,
/// and a round knob 20 across that moves 16 (0.08 s): on, `#3ea6ff` and
/// the bar `#3ea6ff`@0.30; off, `#909090` and white@0.30. Named `name`
/// for screen readers. Takes 36×20.
pub fn toggle(ui: &mut egui::Ui, on: bool, name: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 20.0), Sense::click());
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, name));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_with_time(response.id, on, 0.08);
        let bar = Rect::from_center_size(rect.center(), egui::vec2(36.0, 14.0));
        let (knob, track) = if on {
            (PALETTE.switch, PALETTE.switch.gamma_multiply(0.3))
        } else {
            (PALETTE.quiet, Color32::from_white_alpha(77))
        };
        ui.painter().rect_filled(bar, CornerRadius::same(7), track);
        let centre = egui::pos2(rect.left() + 10.0 + 16.0 * t, rect.center().y);
        // Its shadow, `0 1px 5px` black@0.60, roughly.
        ui.painter().circle_filled(
            centre + egui::vec2(0.0, 1.0),
            11.5,
            Color32::from_black_alpha(60),
        );
        ui.painter().circle_filled(centre, 10.0, knob);
    }
    response
}

/// A pill button with only words: white when `primary`, else quiet.
pub fn pill_button(ui: &mut egui::Ui, text: &str, primary: bool) -> Response {
    let style = if primary { Pill::Filled } else { Pill::Tonal };
    pill(ui, None, text, style)
}

/// A CSS `cubic-bezier(x1, y1, x2, y2)` timing curve at `t` (0 to 1).
pub fn bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let curve = |a: f32, b: f32, s: f32| {
        let u = 1.0 - s;
        3.0 * u * u * s * a + 3.0 * u * s * s * b + s * s * s
    };
    // Find where the curve's x is `t` (bisection; it only grows), then
    // its y there.
    let (mut low, mut high) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let middle = (low + high) / 2.0;
        if curve(x1, x2, middle) < t {
            low = middle;
        } else {
            high = middle;
        }
    }
    curve(y1, y2, (low + high) / 2.0)
}

/// `m:ss` (or `h:mm:ss`).
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_grid_is_youtube_musics() {
        // (window, menu) → (content's left in the window, its width), as
        // measured at 1280 and worked out from the stylesheet elsewhere.
        for (window, menu, x, width) in [
            (960.0, 240.0, 296.0, 596.0),
            (960.0, 72.0, 128.0, 764.0),
            (1150.0, 240.0, 340.0, 698.0),
            (1280.0, 240.0, 340.0, 828.0),
            (1280.0, 72.0, 172.0, 996.0),
            (1536.0, 240.0, 340.0, 1084.0),
            (1920.0, 240.0, 340.0, 1468.0),
            (1920.0, 72.0, 251.0, 1478.0),
        ] {
            let grid = Grid::new(window, window - menu);
            assert_eq!(
                (menu + grid.left, grid.width),
                (x, width),
                "{window} {menu}"
            );
        }
        // Search's column: 860 at 1280, 84 in from the menu.
        let search = Grid::search(1280.0, 1040.0);
        assert_eq!((search.left, search.width), (84.0, 860.0));
    }

    #[test]
    fn cards_and_song_columns_follow_the_window() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        // 5 across at 1280 (146.4, measured), 6 from 1364, 180 below 1150.
        let grid = Grid::new(1280.0, 1040.0);
        let (card, gap) = grid.cards(grid.width, false);
        assert!(close(card, 146.4) && gap == 24.0);
        assert_eq!(grid.song_column(grid.width), 436.0);
        let grid = Grid::new(1440.0, 1200.0);
        assert!(close(grid.cards(grid.width, false).0, 144.666_67));
        let grid = Grid::new(1920.0, 1680.0);
        assert!(close(grid.cards(grid.width, false).0, 224.666_67));
        assert!(close(grid.song_column(grid.width), 473.333_33));
        let grid = Grid::new(960.0, 720.0);
        assert_eq!(grid.cards(grid.width, false), (180.0, 16.0));
        // Small beside an album's header.
        let grid = Grid::new(1280.0, 1040.0);
        assert_eq!(grid.cards(546.0, true), (160.0, 16.0));
    }

    #[test]
    fn bezier_curves_end_where_they_should() {
        let close = |a: f32, b: f32| (a - b).abs() < 0.001;
        assert!(close(bezier(0.2, 0.0, 0.6, 1.0, 0.0), 0.0));
        assert!(close(bezier(0.2, 0.0, 0.6, 1.0, 1.0), 1.0));
        // A straight line stays straight.
        assert!(close(bezier(0.25, 0.25, 0.75, 0.75, 0.3), 0.3));
        // This one starts slowly.
        assert!(bezier(0.2, 0.0, 0.6, 1.0, 0.2) < 0.2);
    }

    #[test]
    fn clock_format() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(65.4), "1:05");
        assert_eq!(clock(3_725.0), "1:02:05");
    }

    #[test]
    fn notes_characters_the_fonts_may_not_draw() {
        let noted = |text: &str| text.chars().any(may_need_script_fonts);
        // Inter draws these, with egui's own emoji.
        for text in [
            "Señorita (feat. Björk) – “Live”…",
            "Αθήνα, Москва",
            "Phở Đặc Biệt",
            "£3 → €5™",
            "Fire 🔥🇧🇷",
            "",
        ] {
            assert!(!noted(text), "{text}");
        }
        // These are looked up in the fonts: most need the computer's fonts
        // (Inter happens to draw the stars and hearts).
        for text in [
            "東京",
            "あいみょん",
            "방탄소년단",
            "عمرو دياب",
            "שלום",
            "हिन्दी",
            "தமிழ்",
            "ไทย",
            "★ Stars ♡",
            "ＦＵＬＬ",
            "𝓢𝓽𝔂𝓵𝓮",
        ] {
            assert!(noted(text), "{text}");
        }
    }
}
