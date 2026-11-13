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

/// Preferences page for recording settings (M10.5).
pub const PREFS_VIDEO_PAGE: &str = "Vídeo";

/// Switch row enabling microphone recording (M10.5).
pub const PREFS_RECORD_AUDIO_LABEL: &str = "Gravar áudio do microfone";

/// Combo row picking which microphone to record (M10.5).
pub const PREFS_MIC_LABEL: &str = "Microfone";

/// First entry of the microphone combo — the OS default input.
pub const MIC_DEFAULT_LABEL: &str = "Padrão do sistema";

/// Toolbar microphone toggle tooltip (M10.5, `Shift+R`).
pub const MIC_TOGGLE_TOOLTIP: &str = "Gravar áudio do microfone (Shift+R)";

/// Toast when the mic cannot be opened at record start — the video is
/// still recorded, just silent (M10.5: recording never blocks on audio).
pub const MIC_UNAVAILABLE: &str = "Microfone indisponível: gravando sem áudio";

/// Toast when the mic dies mid-recording; the file is finalized with
/// both tracks (M10.5 minimum-accepted behavior).
pub const MIC_LOST: &str = "Microfone desconectado: gravação finalizada";

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

/// Side panel group title: software image adjustments (§6.11).
pub const ADJUST_GROUP_TITLE: &str = "Imagem";

/// Side panel / Explorar popover slider labels — same order as
/// `ImageAdjust::KINDS`.
pub const ADJUST_NAMES: [&str; 5] = ["Brilho", "Contraste", "Saturação", "Gama", "Nitidez"];

/// Side panel row: reset the five image adjustments to neutral.
pub const ADJUST_RESET_LABEL: &str = "Restaurar padrões";

/// Explorar toolbar button: opens the brightness/contrast popover.
pub const ADJUST_POPOVER_TOOLTIP: &str = "Ajustes de imagem";

/// Crosshair style names — same order as `CrosshairStyle` variants.
pub const STYLE_NAMES: [&str; 3] = ["Cruz", "Cruz com círculo", "Cruz com escala"];

/// Preferences row: burn grid/crosshair into saved photos (§6.6).
pub const PREFS_BURN_LABEL: &str = "Gravar sobreposições na foto";

/// Preferences row: frame-averaging noise reduction (M10).
pub const PREFS_NOISE_LABEL: &str = "Redução de ruído";

/// Combo choices for `PREFS_NOISE_LABEL` — same order as
/// `NOISE_FRAME_OPTIONS` (off, 4, 8, 16 frames).
pub const NOISE_CHOICES: [&str; 4] = ["Desligada", "4 quadros", "8 quadros", "16 quadros"];

/// Centered notice over the video while frames are averaged (M10).
pub const NOISE_HOLD_STILL: &str = "Mantenha parado…";

/// Toast when the average could not finish: deadline hit.
pub const NOISE_CANCEL_TIMEOUT: &str = "Foto cancelada: tempo esgotado";

/// Toast when the average could not finish: geometry changed.
pub const NOISE_CANCEL_RESIZE: &str = "Foto cancelada: a imagem mudou de tamanho";

/// Toast when the average could not finish: camera went away.
pub const NOISE_CANCEL_DISCONNECTED: &str = "Foto cancelada: câmera desconectada";

/// Maps a noise-capture cancel reason to its toast text (M10).
#[must_use]
pub fn noise_cancelled(reason: crate::services::noise::NoiseCancel) -> &'static str {
    match reason {
        crate::services::noise::NoiseCancel::Timeout => NOISE_CANCEL_TIMEOUT,
        crate::services::noise::NoiseCancel::SizeChanged => NOISE_CANCEL_RESIZE,
        crate::services::noise::NoiseCancel::Disconnected => NOISE_CANCEL_DISCONNECTED,
    }
}

/// Preferences page title: physical Snap button (§6.10).
pub const PREFS_BUTTON_PAGE: &str = "Botão físico";

/// Preferences row: how the button reaches the app.
pub const BUTTON_STATUS_LABEL: &str = "Status";

/// Button connected directly via an evdev input device (kernel driver).
pub const BUTTON_STATUS_EVDEV: &str = "Conectado via driver";

/// Button connected via the `hiview-buttond` service socket.
pub const BUTTON_STATUS_HELPER: &str = "Conectado via serviço";

/// No button transport is delivering events right now.
pub const BUTTON_STATUS_NONE: &str = "Não disponível";

/// Toast when the button transport drops (daemon stop, unplug).
pub const BUTTON_DISCONNECTED_TOAST: &str = "Botão físico desconectado";

/// Preferences row: the press duration that toggles recording.
pub const BUTTON_LONG_PRESS_LABEL: &str = "Toque longo (ms)";

/// Explanatory text under the button group.
pub const BUTTON_GROUP_DESC: &str = "Toque curto tira foto; segurar inicia ou para a gravação. \
     Funciona apenas com o microscópio transmitindo.";

/// Preferences status text for the Snap button source (§6.10).
#[must_use]
pub fn button_status(kind: Option<hiview_button::SourceKind>) -> &'static str {
    match kind {
        Some(hiview_button::SourceKind::Evdev) => BUTTON_STATUS_EVDEV,
        Some(hiview_button::SourceKind::Helper) => BUTTON_STATUS_HELPER,
        None => BUTTON_STATUS_NONE,
    }
}

/// Toast when the button transport connects — makes the active
/// source visible without opening Preferences.
#[must_use]
pub fn button_connected_toast(kind: hiview_button::SourceKind) -> String {
    format!("Botão físico: {}", button_status(Some(kind)))
}

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

/// Side panel group title: measurement tools and calibration (§6.9).
pub const MEASURE_GROUP_TITLE: &str = "Medição";

/// Side panel row: armed measurement tool.
pub const MEASURE_TOOL_LABEL: &str = "Ferramenta";

/// Tool combo entries — same order as the `MeasureTool` arm table
/// (index 0 disarms).
pub const TOOL_NAMES: [&str; 4] = ["Nenhuma", "Régua (L)", "Ângulo (A)", "Círculo (O)"];

/// Per-item row prefix in the measurement list, by tool.
pub const TOOL_SHORT_NAMES: [&str; 3] = ["Régua", "Ângulo", "Círculo"];

/// Side panel row: starts the calibration wizard (draw a line, type
/// the real distance).
pub const CALIBRATE_BUTTON_LABEL: &str = "Nova calibração…";

/// Side panel row: the active calibration profile.
pub const CALIBRATION_ACTIVE_LABEL: &str = "Calibração ativa";

/// First entry of the calibration combo: no profile, pixel results.
pub const CALIBRATION_NONE: &str = "Nenhuma (pixels)";

/// "Nova calibração…" subtitle while the Calibrate tool is armed —
/// the tool combo shows "Nenhuma", so this flags the mode.
pub const CALIBRATION_ARMED_HINT: &str = "Arraste uma linha sobre a distância conhecida.";

/// Side panel button: rename the active calibration profile.
pub const CALIBRATION_RENAME_LABEL: &str = "Renomear…";

/// Side panel button: delete the active calibration profile.
pub const CALIBRATION_DELETE_LABEL: &str = "Excluir…";

/// Side panel row: removes every measurement on screen.
pub const MEASURE_CLEAR_LABEL: &str = "Limpar medições";

/// Permanent banner while a calibration is active (§6.9) — the wording
/// is contractual, keep it exact.
pub const CALIBRATION_BANNER: &str =
    "Calibração válida apenas se o anel de ampliação e a altura não mudaram desde a calibração.";

/// Calibration wizard dialog title.
pub const CALIBRATION_DIALOG_TITLE: &str = "Nova calibração";

/// Calibration wizard: drawn line description ("Linha medida: 123 px").
pub fn calibration_line_label(px: f64) -> String {
    format!("Linha medida: {px:.0} px")
}

/// Calibration wizard row: the real-world length of the drawn line.
pub const CALIBRATION_REAL_LABEL: &str = "Distância real (mm)";

/// Calibration wizard row: profile name entry.
pub const CALIBRATION_NAME_LABEL: &str = "Nome do perfil";

/// Dialog confirm button shared by the calibration wizards.
pub const CALIBRATION_CONFIRM_LABEL: &str = "Criar perfil";

/// Rename dialog title for the active calibration profile.
pub const RENAME_DIALOG_TITLE: &str = "Renomear calibração";

/// Rename dialog confirm button.
pub const RENAME_CONFIRM_LABEL: &str = "Renomear";

/// Delete-profile confirmation title.
pub const CALIBRATION_DELETE_TITLE: &str = "Excluir calibração?";

/// Delete-profile confirmation body ("O perfil X será removido…").
pub fn calibration_delete_body(name: &str) -> String {
    format!(
        "O perfil \"{name}\" será removido. As medições na tela passam a ser exibidas em pixels."
    )
}

/// Generic cancel label for dialogs.
pub const DIALOG_CANCEL: &str = "Cancelar";

/// Generic delete label for confirmations.
pub const DIALOG_DELETE: &str = "Excluir";

/// Toast: a measurement needs a streaming frame to land on.
pub const MEASURE_NO_FRAME: &str = "Meça sobre a imagem da câmera";

/// Grid spacing row title while no calibration is active.
pub const GRID_SPACING_PX_LABEL: &str = "Espaçamento (px)";

/// Grid spacing row title with an active calibration.
pub const GRID_SPACING_MM_LABEL: &str = "Espaçamento (mm)";

/// Default profile name in the calibration wizard.
pub fn calibration_default_name(n: usize) -> String {
    format!("Calibração {n}")
}

/// Toast when the typed profile name already exists.
pub const CALIBRATION_NAME_TAKEN: &str = "Já existe um perfil com esse nome";

#[cfg(test)]
mod tests {
    use super::*;
    use hiview_button::SourceKind;

    /// §6.10: the preferences row maps each source kind (and its
    /// absence) to the PT-BR status the user reads.
    #[test]
    fn button_status_maps_every_kind() {
        assert_eq!(button_status(Some(SourceKind::Evdev)), BUTTON_STATUS_EVDEV);
        assert_eq!(
            button_status(Some(SourceKind::Helper)),
            BUTTON_STATUS_HELPER
        );
        assert_eq!(button_status(None), BUTTON_STATUS_NONE);
    }
}
