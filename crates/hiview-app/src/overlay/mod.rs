// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 MundoTela Tecnologia

//! Overlay rendering (docs/ARCHITECTURE.md §6.9, ADR-012): a single
//! cairo renderer draws the grid and the crosshair, used both by the
//! `Viewport` snapshot hook and by the photo burn-in. `tools/` is
//! reserved for the M8 measurement tools.

mod burn;
mod renderer;

pub use burn::burn_in;
pub use renderer::OverlayRenderer;
