//! Root application component.

use relm4::adw::prelude::*;
use relm4::{ComponentParts, ComponentSender, SimpleComponent};

use crate::strings;

/// Root component of the application.
///
/// For now (M0) it only shows the "searching for microscope" placeholder
/// screen; real camera states arrive in M1.
pub struct AppModel;

#[relm4::component(pub)]
impl SimpleComponent for AppModel {
    type Init = ();
    type Input = ();
    type Output = ();

    view! {
        adw::ApplicationWindow {
            set_title: Some(strings::APP_TITLE),
            set_default_width: 800,
            set_default_height: 600,

            #[wrap(Some)]
            set_content = &adw::ToolbarView {
                add_top_bar = &adw::HeaderBar {},

                #[wrap(Some)]
                set_content = &adw::StatusPage {
                    set_icon_name: Some(strings::SEARCHING_ICON),
                    set_title: strings::SEARCHING_TITLE,
                    set_description: Some(strings::SEARCHING_DESCRIPTION),
                },
            },
        }
    }

    fn init(
        _init: Self::Init,
        root: Self::Root,
        _sender: ComponentSender<Self>,
    ) -> ComponentParts<Self> {
        let model = Self;
        let widgets = view_output!();
        ComponentParts { model, widgets }
    }
}
