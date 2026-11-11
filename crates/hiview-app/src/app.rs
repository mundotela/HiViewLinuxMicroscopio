// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Root application component (docs/CONVENTIONS.md §4).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hiview_capture::{CameraEvent, CameraState};
use relm4::actions::{RelmAction, RelmActionGroup};
use relm4::adw::prelude::*;
use relm4::gtk::{self, gio};
use relm4::{ComponentParts, ComponentSender, SimpleComponent, adw};

use crate::cli::Args;
use crate::services::camera::CameraService;
use crate::strings;
use crate::widgets::viewport;

/// Stylesheet for the FPS badge, installed once at startup.
pub const GLOBAL_CSS: &str = ".fps-indicator {
    background-color: rgba(0, 0, 0, 0.65);
    color: #ffffff;
    padding: 4px 10px;
    margin: 12px;
    border-radius: 8px;
    font-weight: bold;
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

/// Inputs handled by the root component.
#[derive(Debug)]
pub enum Input {
    /// A camera event forwarded from the monitor/bus threads.
    Camera(CameraEvent),
    /// At least one new frame is pending in the frame bus.
    FrameReady,
    /// The user picked an entry in the camera menu.
    SelectCamera(String),
}

/// Root component of the application.
pub struct AppModel {
    camera: CameraService,
    state: CameraState,
    // Widget handles stored for imperative updates (frame upload, page
    // switching). All UI *state* lives in the fields above.
    content_stack: gtk::Stack,
    searching_page: adw::StatusPage,
    disconnected_page: adw::StatusPage,
    error_page: adw::StatusPage,
    video_page: gtk::Overlay,
    viewport: gtk::Picture,
    menu_model: gio::Menu,
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
                        set_child = &gtk::Picture {
                            set_content_fit: gtk::ContentFit::Contain,
                            set_can_shrink: true,
                            set_hexpand: true,
                            set_vexpand: true,
                        },

                        add_overlay = &gtk::Label {
                            set_halign: gtk::Align::End,
                            set_valign: gtk::Align::Start,
                            set_css_classes: &["fps-indicator"],
                            #[watch]
                            set_visible: model.state == CameraState::Streaming,
                            #[watch]
                            set_label: &model.fps_label,
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
            content_stack: gtk::Stack::new(),
            searching_page: adw::StatusPage::new(),
            disconnected_page: adw::StatusPage::new(),
            error_page: adw::StatusPage::new(),
            video_page: gtk::Overlay::new(),
            viewport: gtk::Picture::new(),
            menu_model: gio::Menu::new(),
            fps_label: String::new(),
            frame_times: VecDeque::new(),
        };
        model.rebuild_menu();

        let widgets = view_output!();
        model.grab_widgets(&widgets);

        let select_camera = RelmAction::<SelectCameraAction>::new_stateful_with_target_value(
            &model.camera.selection_target(),
            {
                let sender = sender.clone();
                move |_, state, value: String| {
                    sender.input(Input::SelectCamera(value.clone()));
                    *state = value;
                }
            },
        );
        let mut group = RelmActionGroup::<WindowActionGroup>::new();
        group.add_action(select_camera);
        group.register_for_widget(root);

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
                    viewport::set_frame(&self.viewport, &frame);
                    self.tick_fps();
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
            viewport::clear(&self.viewport);
            self.frame_times.clear();
            self.fps_label.clear();
        }
        self.rebuild_menu();
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
            let label = if camera.is_microscope {
                strings::MICROSCOPE_LABEL.to_string()
            } else {
                // Friendly product name; the node is appended only to tell
                // apart cameras with identical names.
                let duplicate = self
                    .camera
                    .cameras()
                    .iter()
                    .filter(|c| c.display_name() == camera.display_name())
                    .count()
                    > 1;
                if duplicate {
                    format!("{} ({})", camera.display_name(), camera.dev_node.display())
                } else {
                    camera.display_name().to_string()
                }
            };
            cameras_menu.append_item(
                &RelmAction::<SelectCameraAction>::to_menu_item_with_target_value(
                    &label,
                    &camera.stable_id(),
                ),
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
            self.fps_label = label;
        }
    }
}
