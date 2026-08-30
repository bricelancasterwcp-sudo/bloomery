// bloomery — an operating layer for local LLMs.
// Copyright (C) 2026 Brice Lancaster
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU Affero General Public License, version 3, as
// published by the Free Software Foundation.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or
// FITNESS FOR A PARTICULAR PURPOSE. See the GNU Affero General Public License
// for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.
//
// Commercial licensing is available as an alternative to the AGPL — see
// LICENSING.md.

pub mod agents;
pub mod api_memory;
mod api_native;
mod api_task;
mod api_v1;
pub mod codec_probe;
pub mod config;
pub mod drift;
pub mod http;
#[cfg(feature = "llama")]
pub mod llama_send;
pub mod memory;
pub mod pager;
pub mod post;
pub mod swap;
pub mod task;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
