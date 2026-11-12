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

/// Tooltip of the shutter toolbar button (`Espaço`).
pub const SHUTTER_TOOLTIP: &str = "Tirar foto (Espaço)";

/// Toast shown after a photo is written.
pub const PHOTO_SAVED: &str = "Foto salva";

/// Toast shown when a photo could not be written.
pub const PHOTO_SAVE_FAILED: &str = "Não foi possível salvar a foto";

/// Toast button that opens the saved photo.
pub const OPEN_BUTTON: &str = "Abrir";

/// Toast shown when the config file was invalid and reset (§6.12).
pub const CONFIG_RECOVERED: &str = "Configuração inválida — padrões restaurados";

/// Label of the preferences menu item and dialog (`Ctrl+,`).
pub const PREFERENCES_LABEL: &str = "Preferências";

/// Title of the "Fotos" preferences page.
pub const PREFS_PHOTOS_PAGE: &str = "Fotos";

/// Label of the format combo in preferences.
pub const PREFS_FORMAT_LABEL: &str = "Formato";

/// Label of the resolution combo in preferences.
pub const PREFS_RESOLUTION_LABEL: &str = "Resolução";

/// Label of the folder row in preferences.
pub const PREFS_FOLDER_LABEL: &str = "Pasta";

/// Button that opens the folder picker in preferences.
pub const PREFS_FOLDER_BUTTON: &str = "Alterar…";

/// Resolution option: sensor native size.
pub const RES_NATIVE_LABEL: &str = "Nativa (640×480)";

/// Resolution option: 2× interpolated.
pub const RES_1280_LABEL: &str = "1280×960 (interpolada)";

/// Resolution option: 2.5× interpolated.
pub const RES_1600_LABEL: &str = "1600×1200 (interpolada)";

/// Toast shown when the photo queue is full and a shot is dropped.
pub const PHOTO_BUSY: &str = "Salvando fotos, aguarde…";

/// Tooltip of the photo-mode toolbar toggle.
pub const MODE_PHOTO_TOOLTIP: &str = "Modo foto";

/// Tooltip of the video-mode toolbar toggle.
pub const MODE_VIDEO_TOOLTIP: &str = "Modo vídeo — aperte R para gravar";

/// Tooltip of the video-mode toggle when no H.264 encoder exists.
pub const MODE_VIDEO_UNAVAILABLE_TOOLTIP: &str =
    "Gravação indisponível: codificador H.264 não encontrado";

/// Tooltip of the shutter button in video mode, idle (`R`).
pub const RECORD_START_TOOLTIP: &str = "Iniciar gravação (R)";

/// Tooltip of the shutter button while recording (`R`).
pub const RECORD_STOP_TOOLTIP: &str = "Parar gravação (R)";

/// Toast shown after a video is written.
pub const VIDEO_SAVED: &str = "Vídeo salvo";

/// Toast shown when a video could not be finalized.
pub const VIDEO_SAVE_FAILED: &str = "Não foi possível salvar o vídeo";

/// Toast shown when recording ends because the camera disappeared.
pub const REC_ENDED_DISCONNECTED: &str = "Gravação encerrada: microscópio desconectado";

/// Toast shown when the disk-space gate refuses a recording.
pub const REC_LOW_DISK: &str = "Espaço em disco insuficiente para gravar (mínimo 500 MB)";

/// Toast shown when no H.264 encoder is installed at all.
pub const REC_UNSUPPORTED: &str = "Gravação indisponível: codificador H.264 não encontrado";

/// Header button that opens the gallery (`Ctrl+G`).
pub const GALLERY_BUTTON_LABEL: &str = "Galeria";

/// Tooltip of the gallery header button.
pub const GALLERY_BUTTON_TOOLTIP: &str = "Abrir a galeria (Ctrl+G)";

/// Title of the gallery page.
pub const GALLERY_TITLE: &str = "Galeria";

/// Symbolic icon of the empty-gallery status page.
pub const GALLERY_EMPTY_ICON: &str = "folder-pictures-symbolic";

/// Status page title when the gallery has nothing to show.
pub const GALLERY_EMPTY_TITLE: &str = "Galeria vazia";

/// Status page description when the gallery has nothing to show.
pub const GALLERY_EMPTY_DESCRIPTION: &str = "Fotos e vídeos feitos com o HiView aparecem aqui.";

/// Context action: open the file with the system app.
pub const GALLERY_OPEN: &str = "Abrir";

/// Context action: reveal the file in its folder.
pub const GALLERY_SHOW_IN_FOLDER: &str = "Mostrar na pasta";

/// Context action: move the file to the trash.
pub const GALLERY_TRASH: &str = "Mover para a lixeira";

/// Heading of the trash confirmation dialog.
pub const TRASH_CONFIRM_TITLE: &str = "Mover para a lixeira?";

/// Destructive response of the trash confirmation dialog.
pub const TRASH_CONFIRM_BUTTON: &str = "Mover para a lixeira";

/// Generic cancel response.
pub const CANCEL_BUTTON: &str = "Cancelar";

/// Toast shown after a file was moved to the trash.
pub const TRASHED: &str = "Movido para a lixeira";

/// Toast shown when trashing failed.
pub const TRASH_FAILED: &str = "Não foi possível mover para a lixeira";

/// Toast shown when the system app could not open the file.
pub const OPEN_FAILED: &str = "Não foi possível abrir o arquivo";

/// Label of the in-progress recording tile.
pub const RECORDING_TILE: &str = "Gravando…";

/// Header mode toggle: clean UI for casual exploring.
pub const MODE_EXPLORE_LABEL: &str = "Explorar";

/// Header mode toggle: electronics workbench UI.
pub const MODE_BENCH_LABEL: &str = "Bancada";

/// Tooltip of both mode toggles (`M`).
pub const MODE_TOGGLE_TOOLTIP: &str = "Alternar Explorar/Bancada (M)";

/// Toast when `G` or `C` is pressed in Explorar mode (§13).
pub const BENCH_ONLY: &str = "Disponível no modo Bancada";

/// Title of the Bancada side panel header.
pub const BENCH_PANEL_TITLE: &str = "Bancada";

/// Side panel group title: grid settings.
pub const GRID_GROUP_TITLE: &str = "Grade";

/// Side panel row: grid on/off (`G`).
pub const GRID_SHOW_LABEL: &str = "Mostrar grade (G)";

/// Side panel row: grid spacing in image pixels.
pub const GRID_SPACING_LABEL: &str = "Espaçamento (px)";

/// Side panel row: grid line color.
pub const GRID_COLOR_LABEL: &str = "Cor";

/// Side panel row: grid line opacity.
pub const GRID_OPACITY_LABEL: &str = "Opacidade";

/// Side panel group title: crosshair settings.
pub const CROSS_GROUP_TITLE: &str = "Mira";

/// Side panel row: crosshair on/off (`C`).
pub const CROSS_SHOW_LABEL: &str = "Mostrar mira (C)";

/// Side panel row: crosshair style.
pub const CROSS_STYLE_LABEL: &str = "Estilo";

/// Side panel row: crosshair color.
pub const CROSS_COLOR_LABEL: &str = "Cor";

/// Color preset names — same order as `OverlayColor::PRESETS`.
pub const COLOR_NAMES: [&str; 5] = ["Verde", "Amarelo", "Ciano", "Branco", "Vermelho"];

/// Crosshair style names — same order as `CrosshairStyle` variants.
pub const STYLE_NAMES: [&str; 3] = ["Cruz", "Cruz com círculo", "Cruz com escala"];

/// Preferences row: burn grid/crosshair into saved photos (§6.6).
pub const PREFS_BURN_LABEL: &str = "Gravar sobreposições na foto";

/// Body of the trash confirmation dialog.
pub fn trash_confirm_body(name: &str) -> String {
    format!("\"{name}\" será movido para a lixeira. Você pode recuperá-lo depois.")
}

/// Dropped-frame counter shown with `--debug-overlay` while recording.
pub fn drops_label(dropped: u64) -> String {
    format!("{dropped} quadros perdidos")
}

/// Elapsed recording time as `M:SS` (or `H:MM:SS` past one hour).
pub fn elapsed_label(seconds: u64) -> String {
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds % 3600 / 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}
