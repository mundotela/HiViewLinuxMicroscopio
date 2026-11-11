// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Root application component (docs/CONVENTIONS.md §4).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hiview_capture::{CameraDescriptor, CameraEvent, CameraState};
use hiview_core::ViewOrientation;
use relm4::actions::{AccelsPlus, RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::gtk::{self, gio};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw};

use crate::cli::Args;
use crate::services::camera::CameraService;
use crate::strings;
use crate::widgets::viewport::Viewport;

/// Stylesheet for the FPS badge, freeze indicator and floating toolbar,
/// installed once at startup.
pub const GLOBAL_CSS: &str = ".fps-indicator {
    background-color: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    padding: 4px 10px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
}
.frozen-view {
    border: 4px solid #e5a50a;
}
.frozen-badge {
    background-color: rgba(0, 0, 0, 0.65);
    color: #f6d32d;
    padding: 6px 14px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
}
.floating-toolbar {
    background-color: rgba(0, 0, 0, 0.6);
    border-radius: 24px;
    padding: 4px;
    margin-bottom: 16px;
}";

/// Menu target value meaning "pick automatically".
const AUTO_TARGET: &str = "auto";

relm4::new_action_group!(WindowActionGroup, "win");
relm4::new_stateful_action!(
    SelectCameraAction,
    WindowActionGroup,
    "select-camera",
    String,
    String
);
relm4::new_stateless_action!(ZoomInAction, WindowActionGroup, "zoom-in");
relm4::new_stateless_action!(ZoomOutAction, WindowActionGroup, "zoom-out");
relm4::new_stateless_action!(FitViewAction, WindowActionGroup, "fit-view");
relm4::new_stateless_action!(Zoom100Action, WindowActionGroup, "zoom-100");
relm4::new_stateless_action!(MirrorHAction, WindowActionGroup, "mirror-h");
relm4::new_stateless_action!(MirrorVAction, WindowActionGroup, "mirror-v");
relm4::new_stateless_action!(RotateCcwAction, WindowActionGroup, "rotate-ccw");
relm4::new_stateless_action!(RotateCwAction, WindowActionGroup, "rotate-cw");
relm4::new_stateless_action!(FreezeAction, WindowActionGroup, "freeze");
relm4::new_stateless_action!(FullscreenAction, WindowActionGroup, "fullscreen");
relm4::new_stateless_action!(EscapeAction, WindowActionGroup, "escape");

/// Inputs handled by the root component.
#[derive(Debug)]
pub enum Input {
    /// A camera event forwarded from the monitor/bus threads.
    Camera(CameraEvent),
    /// At least one new frame is pending in the frame bus.
    FrameReady,
    /// The user picked an entry in the camera menu.
    SelectCamera(String),
    /// Zoom in around the widget center (`+`).
    ZoomIn,
    /// Zoom out around the widget center (`-`).
    ZoomOut,
    /// Fit the frame to the window (`0`).
    FitView,
    /// Show the frame at native size (`1`).
    Zoom100,
    /// Toggle horizontal mirroring (`H`).
    MirrorH,
    /// Toggle vertical mirroring (`V`).
    MirrorV,
    /// Rotate 90° counterclockwise (`[`).
    RotateCcw,
    /// Rotate 90° clockwise (`]`).
    RotateCw,
    /// Freeze/unfreeze the displayed frame (`F`); the stream keeps running.
    ToggleFreeze,
    /// Toggle fullscreen (`F11`).
    ToggleFullscreen,
    /// Leave fullscreen (`Esc`).
    Escape,
}

/// Root component of the application.
pub struct AppModel {
    camera: CameraService,
    state: CameraState,
    /// Mirror/rotation toggles, forwarded to the pipeline's `videoflip`.
    orientation: ViewOrientation,
    /// View-only freeze: the pipeline keeps streaming underneath.
    frozen: bool,
    /// Window handle for fullscreen toggling.
    window: adw::ApplicationWindow,
    // Widget handles stored for imperative updates (frame upload, page
    // switching). All UI *state* lives in the fields above.
    content_stack: gtk::Stack,
    searching_page: adw::StatusPage,
    disconnected_page: adw::StatusPage,
    error_page: adw::StatusPage,
    video_page: gtk::Overlay,
    viewport: Viewport,
    menu_model: gio::Menu,
    /// Whether the FPS badge is enabled (debug builds or `--debug-overlay`).
    show_fps: bool,
    fps_label: String,
    frame_times: VecDeque<Instant>,
}

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = Args;
    type Input = Input;
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some(strings::APP_TITLE),
            set_default_width: 960,
            set_default_height: 720,

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {
                    pack_end = &gtk::MenuButton {
                        set_icon_name: "open-menu-symbolic",
                        set_tooltip_text: Some(strings::CAMERA_MENU_TOOLTIP),
                        #[watch]
                        set_visible: model.camera.has_camera_menu(),
                        #[watch]
                        set_menu_model: Some(&model.menu_model),
                    },
                },

                #[name = "content_stack"]
                #[wrap(Some)]
                set_content = &gtk::Stack {

                    #[name = "searching_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::SEARCHING_ICON),
                        set_title: strings::SEARCHING_TITLE,
                        set_description: Some(strings::SEARCHING_DESCRIPTION),
                    },

                    #[name = "disconnected_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::DISCONNECTED_ICON),
                        set_title: strings::DISCONNECTED_TITLE,
                        set_description: Some(strings::DISCONNECTED_DESCRIPTION),
                    },

                    #[name = "error_page"]
                    add_child = &adw::StatusPage {
                        set_icon_name: Some(strings::ERROR_ICON),
                        set_title: strings::ERROR_TITLE,
                        set_description: Some(strings::ERROR_DESCRIPTION),
                    },

                    #[name = "video_page"]
                    add_child = &gtk::Overlay {

                        #[name = "viewport"]
                        #[wrap(Some)]
                        set_child = &Viewport {
                        },

                        add_overlay = &gtk::Label {
                            set_halign: gtk::Align::End,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["fps-indicator"],
                            #[watch]
                            set_visible: model.show_fps
                                && model.state == CameraState::Streaming,
                            #[watch]
                            set_label: &model.fps_label,
                        },

                        add_overlay = &gtk::Label {
                            set_label: strings::FROZEN_BADGE,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["frozen-badge"],
                            #[watch]
                            set_visible: model.frozen,
                        },

                        add_overlay = &gtk::Box {
                            set_orientation: gtk::Orientation::Horizontal,
                            set_spacing: 4,
                            set_halign: gtk::Align::Center,
                            set_valign: gtk::Align::End,
                            set_css_classes: &["floating-toolbar"],

                            gtk::Button {
                                set_icon_name: "zoom-out-symbolic",
                                set_tooltip_text: Some(strings::ZOOM_OUT_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::ZoomOut,
                            },
                            gtk::Button {
                                set_icon_name: "zoom-in-symbolic",
                                set_tooltip_text: Some(strings::ZOOM_IN_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::ZoomIn,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Button {
                                set_icon_name: "object-flip-horizontal-symbolic",
                                set_tooltip_text: Some(strings::MIRROR_H_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::MirrorH,
                            },
                            gtk::Button {
                                set_icon_name: "object-flip-vertical-symbolic",
                                set_tooltip_text: Some(strings::MIRROR_V_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::MirrorV,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::Button {
                                set_icon_name: "object-rotate-left-symbolic",
                                set_tooltip_text: Some(strings::ROTATE_CCW_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::RotateCcw,
                            },
                            gtk::Button {
                                set_icon_name: "object-rotate-right-symbolic",
                                set_tooltip_text: Some(strings::ROTATE_CW_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::RotateCw,
                            },
                            gtk::Separator {
                                set_orientation: gtk::Orientation::Vertical,
                            },
                            gtk::ToggleButton {
                                set_icon_name: "weather-snow-symbolic",
                                set_tooltip_text: Some(strings::FREEZE_TOOLTIP),
                                set_has_frame: false,
                                connect_clicked => Input::ToggleFreeze,
                                #[watch]
                                set_active: model.frozen,
                            },
                        },
                    },
                },
            },
        }
    }

    fn init(
        init: Self::Init,
        root: Self::Root,
        sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        if init.debug_overlay
            && let Some(settings) = gtk::Settings::default()
        {
            settings.set_property("gtk-debug-overlay", true);
        }

        let on_event: Arc<dyn Fn(CameraEvent) + Send + Sync> = {
            let sender = sender.clone();
            Arc::new(move |event| sender.input(Input::Camera(event)))
        };

        let camera = CameraService::new(&init.source, on_event);
        camera.frame_bus().set_ui_notify(Box::new({
            let sender = sender.clone();
            move || sender.input(Input::FrameReady)
        }));

        let mut model = Self {
            state: camera.state(),
            camera,
            orientation: ViewOrientation::default(),
            frozen: false,
            window: root.clone(),
            content_stack: gtk::Stack::new(),
            searching_page: adw::StatusPage::new(),
            disconnected_page: adw::StatusPage::new(),
            error_page: adw::StatusPage::new(),
            video_page: gtk::Overlay::new(),
            viewport: Viewport::new(),
            menu_model: gio::Menu::new(),
            show_fps: cfg!(debug_assertions) || init.debug_overlay,
            fps_label: String::new(),
            frame_times: VecDeque::new(),
        };
        model.rebuild_menu();

        let widgets = view_output!();
        model.grab_widgets(&widgets);

        let mut group = RelmActionGroup::<WindowActionGroup>::new();
        group.add_action(
            RelmAction::<SelectCameraAction>::new_stateful_with_target_value(
                &model.camera.selection_target(),
                {
                    let sender = sender.clone();
                    move |_, state, value: String| {
                        sender.input(Input::SelectCamera(value.clone()));
                        *state = value;
                    }
                },
            ),
        );
        macro_rules! view_action {
            ($action:ident, $input:expr) => {
                group.add_action(RelmAction::<$action>::new_stateless({
                    let sender = sender.clone();
                    move |_| sender.input($input)
                }));
            };
        }
        view_action!(ZoomInAction, Input::ZoomIn);
        view_action!(ZoomOutAction, Input::ZoomOut);
        view_action!(FitViewAction, Input::FitView);
        view_action!(Zoom100Action, Input::Zoom100);
        view_action!(MirrorHAction, Input::MirrorH);
        view_action!(MirrorVAction, Input::MirrorV);
        view_action!(RotateCcwAction, Input::RotateCcw);
        view_action!(RotateCwAction, Input::RotateCw);
        view_action!(FreezeAction, Input::ToggleFreeze);
        view_action!(FullscreenAction, Input::ToggleFullscreen);
        view_action!(EscapeAction, Input::Escape);
        group.register_for_widget(root);

        // Shortcuts for the M2 actions (docs/CONVENTIONS.md §13).
        let app = relm4::main_adw_application();
        app.set_accelerators_for_action::<ZoomInAction>(&["plus", "equal", "KP_Add"]);
        app.set_accelerators_for_action::<ZoomOutAction>(&["minus", "KP_Subtract"]);
        app.set_accelerators_for_action::<FitViewAction>(&["0", "KP_0"]);
        app.set_accelerators_for_action::<Zoom100Action>(&["1", "KP_1"]);
        app.set_accelerators_for_action::<MirrorHAction>(&["h", "H"]);
        app.set_accelerators_for_action::<MirrorVAction>(&["v", "V"]);
        app.set_accelerators_for_action::<RotateCcwAction>(&["bracketleft"]);
        app.set_accelerators_for_action::<RotateCwAction>(&["bracketright"]);
        app.set_accelerators_for_action::<FreezeAction>(&["f", "F"]);
        app.set_accelerators_for_action::<FullscreenAction>(&["F11"]);
        app.set_accelerators_for_action::<EscapeAction>(&["Escape"]);

        ComponentParts { model, widgets }
    }

    fn update(&mut self, message: Self::Input, _sender: ComponentSender<Self>) {
        match message {
            Input::Camera(event) => {
                self.camera.on_event(event);
                self.on_state_change();
            }
            Input::SelectCamera(target) => {
                self.camera.select(&target);
                self.on_state_change();
            }
            Input::FrameReady => {
                if let Some(frame) = self.camera.frame_bus().take_latest_for_ui() {
                    self.viewport.set_frame(frame);
                    if self.show_fps {
                        self.tick_fps();
                    }
                }
            }
            Input::ZoomIn => self.viewport.zoom_in(),
            Input::ZoomOut => self.viewport.zoom_out(),
            Input::FitView => self.viewport.fit(),
            Input::Zoom100 => self.viewport.zoom_100(),
            Input::MirrorH => {
                self.orientation.toggle_mirror_h();
                self.apply_orientation();
            }
            Input::MirrorV => {
                self.orientation.toggle_mirror_v();
                self.apply_orientation();
            }
            Input::RotateCcw => {
                self.orientation.rotate_ccw();
                self.apply_orientation();
            }
            Input::RotateCw => {
                self.orientation.rotate_cw();
                self.apply_orientation();
            }
            Input::ToggleFreeze => {
                if self.state == CameraState::Streaming {
                    self.set_frozen(!self.frozen);
                }
            }
            Input::ToggleFullscreen => {
                if self.window.is_fullscreen() {
                    self.window.unfullscreen();
                } else {
                    self.window.fullscreen();
                }
            }
            Input::Escape => {
                if self.window.is_fullscreen() {
                    self.window.unfullscreen();
                }
            }
        }
    }
}

impl AppModel {
    /// Mirrors the widget handles the component needs to drive imperatively.
    fn grab_widgets(&mut self, widgets: &<Self as SimpleComponent>::Widgets) {
        self.content_stack = widgets.content_stack.clone();
        self.searching_page = widgets.searching_page.clone();
        self.disconnected_page = widgets.disconnected_page.clone();
        self.error_page = widgets.error_page.clone();
        self.video_page = widgets.video_page.clone();
        self.viewport = widgets.viewport.clone();
    }

    /// Syncs derived UI after a camera state change.
    fn on_state_change(&mut self) {
        self.state = self.camera.state();
        let page: &gtk::Widget = match self.state {
            CameraState::Searching | CameraState::Starting => self.searching_page.upcast_ref(),
            CameraState::Disconnected => self.disconnected_page.upcast_ref(),
            CameraState::Error => self.error_page.upcast_ref(),
            CameraState::Streaming => self.video_page.upcast_ref(),
        };
        self.content_stack.set_visible_child(page);
        if self.state != CameraState::Streaming {
            self.viewport.reset();
            self.set_frozen(false);
            self.frame_times.clear();
            self.fps_label.clear();
        }
        self.rebuild_menu();
    }

    /// Forwards the current mirror/rotation to the capture pipeline.
    fn apply_orientation(&mut self) {
        if self.state == CameraState::Streaming {
            self.camera.set_orientation(self.orientation);
        }
    }

    /// Toggles the view-only freeze and its visual indicator.
    fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
        self.viewport.set_frozen(frozen);
        if frozen {
            self.video_page.add_css_class("frozen-view");
        } else {
            self.video_page.remove_css_class("frozen-view");
        }
    }

    /// Menu entry (label, action target) for one camera (pure, unit-tested):
    /// the microscope keeps its fixed label, other cameras show the
    /// friendly product name with the node appended only when names
    /// collide; the target is always the stable id, never the /dev node.
    fn camera_menu_entry(
        camera: &CameraDescriptor,
        cameras: &[CameraDescriptor],
    ) -> (String, String) {
        let label = if camera.is_microscope {
            strings::MICROSCOPE_LABEL.to_string()
        } else if cameras
            .iter()
            .filter(|c| c.display_name() == camera.display_name())
            .count()
            > 1
        {
            format!("{} ({})", camera.display_name(), camera.dev_node.display())
        } else {
            camera.display_name().to_string()
        };
        (label, camera.stable_id())
    }

    /// Rebuilds the camera selection menu from the current camera list.
    fn rebuild_menu(&mut self) {
        let cameras_menu = gio::Menu::new();
        cameras_menu.append_item(
            &RelmAction::<SelectCameraAction>::to_menu_item_with_target_value(
                strings::CAMERA_AUTO_LABEL,
                &AUTO_TARGET.to_string(),
            ),
        );
        for camera in self.camera.cameras() {
            let (label, target) = Self::camera_menu_entry(camera, self.camera.cameras());
            cameras_menu.append_item(
                &RelmAction::<SelectCameraAction>::to_menu_item_with_target_value(&label, &target),
            );
        }
        let menu = gio::Menu::new();
        menu.append_submenu(Some(strings::CAMERA_SUBMENU_LABEL), &cameras_menu);
        self.menu_model = menu;
    }

    /// Sliding-window FPS counter over the last second of rendered frames.
    fn tick_fps(&mut self) {
        let now = Instant::now();
        self.frame_times.push_back(now);
        while self
            .frame_times
            .front()
            .is_some_and(|t| now.duration_since(*t) > Duration::from_secs(1))
        {
            self.frame_times.pop_front();
        }
        let label = format!("{} fps", self.frame_times.len());
        if label != self.fps_label {
            tracing::debug!("rendered: {label}");
            self.fps_label = label;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn fake_camera(name: &str, node: &str, usb_path: Option<&str>) -> CameraDescriptor {
        CameraDescriptor {
            dev_node: PathBuf::from(node),
            name: name.into(),
            usb_vid: Some(0x1111),
            usb_pid: Some(0x2222),
            usb_path: usb_path.map(str::to_string),
            is_microscope: false,
            modes: Vec::new(),
        }
    }

    /// Regression for the M1 review bug: the menu action target must be
    /// the stable id (usb path), never the /dev node that selected_for
    /// cannot match.
    #[test]
    fn menu_target_is_stable_id_not_device_node() {
        let cam = fake_camera("HD camera", "/dev/video2", Some("pci-usb-0:5"));
        let (_, target) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam));
        assert_eq!(target, "pci-usb-0:5");
        assert_ne!(target, cam.dev_node.to_string_lossy());
    }

    #[test]
    fn menu_label_is_friendly_name_without_node() {
        let cam = fake_camera("ACER HD User Facing: ACER HD Us", "/dev/video0", Some("p"));
        let (label, _) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam));
        assert_eq!(label, "ACER HD User Facing");
    }

    #[test]
    fn menu_label_appends_node_only_for_duplicate_names() {
        let a = fake_camera("HD camera", "/dev/video0", Some("p1"));
        let b = fake_camera("HD camera", "/dev/video2", Some("p2"));
        let cams = [a.clone(), b];
        let (label, _) = AppModel::camera_menu_entry(&a, &cams);
        assert_eq!(label, "HD camera (/dev/video0)");
    }

    #[test]
    fn microscope_keeps_fixed_label() {
        let mut cam = fake_camera("HD camera", "/dev/video2", Some("p"));
        cam.is_microscope = true;
        let (label, _) = AppModel::camera_menu_entry(&cam, std::slice::from_ref(&cam));
        assert_eq!(label, strings::MICROSCOPE_LABEL);
    }
}
