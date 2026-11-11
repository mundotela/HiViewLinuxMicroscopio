// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! All user-visible strings, in Brazilian Portuguese (docs/CONVENTIONS.md §1).
//! Centralized here to ease future localization.

/// Application ID registered with GTK/GIO.
pub const APP_ID: &str = "br.com.mundotela.HiView";

/// Main window title.
pub const APP_TITLE: &str = "HiView";

/// Symbolic icon shown while searching for the microscope.
pub const SEARCHING_ICON: &str = "camera-web-symbolic";

/// Status page title while searching for the microscope.
pub const SEARCHING_TITLE: &str = "Procurando microscópio…";

/// Status page description while searching for the microscope.
pub const SEARCHING_DESCRIPTION: &str = "Conecte o microscópio a uma porta USB.";

/// Symbolic icon shown when the active camera was unplugged.
pub const DISCONNECTED_ICON: &str = "camera-disabled-symbolic";

/// Status page title when the active camera was unplugged.
pub const DISCONNECTED_TITLE: &str = "Microscópio desconectado";

/// Status page description when the active camera was unplugged.
pub const DISCONNECTED_DESCRIPTION: &str = "Reconecte o microscópio à porta USB.";

/// Symbolic icon shown on a fatal capture error.
pub const ERROR_ICON: &str = "dialog-error-symbolic";

/// Status page title on a fatal capture error.
pub const ERROR_TITLE: &str = "Não foi possível iniciar a câmera";

/// Status page description on a fatal capture error.
pub const ERROR_DESCRIPTION: &str =
    "Verifique se o microscópio está conectado e se nenhum outro programa está usando a câmera.";

/// Tooltip of the camera selection menu button.
pub const CAMERA_MENU_TOOLTIP: &str = "Escolher câmera";

/// Menu item that restores automatic microscope detection.
pub const CAMERA_AUTO_LABEL: &str = "Automático";

/// Title of the camera submenu (docs/USER_MANUAL.md).
pub const CAMERA_SUBMENU_LABEL: &str = "Câmera";

/// Menu label for the known microscope.
pub const MICROSCOPE_LABEL: &str = "Microscópio (05e3:f12a)";

/// Tooltip of the zoom-in toolbar button (`+`).
pub const ZOOM_IN_TOOLTIP: &str = "Aproximar (+)";

/// Tooltip of the zoom-out toolbar button (`-`).
pub const ZOOM_OUT_TOOLTIP: &str = "Afastar (-)";

/// Tooltip of the horizontal-mirror toolbar button (`H`).
pub const MIRROR_H_TOOLTIP: &str = "Espelhar horizontalmente (H)";

/// Tooltip of the vertical-mirror toolbar button (`V`).
pub const MIRROR_V_TOOLTIP: &str = "Espelhar verticalmente (V)";

/// Tooltip of the rotate-counterclockwise toolbar button (`[`).
pub const ROTATE_CCW_TOOLTIP: &str = "Girar 90° para a esquerda ([)";

/// Tooltip of the rotate-clockwise toolbar button (`]`).
pub const ROTATE_CW_TOOLTIP: &str = "Girar 90° para a direita (])";

/// Tooltip of the freeze toolbar button (`F`).
pub const FREEZE_TOOLTIP: &str = "Congelar a imagem (F)";

/// Badge shown on top of the frozen frame.
pub const FROZEN_BADGE: &str = "CONGELADO";
