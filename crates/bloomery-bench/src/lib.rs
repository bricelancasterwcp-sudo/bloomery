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

//! The G2 instrument: a driver that makes the daemon page agents in and out,
//! and a pure reader that turns the journal it left behind into the gate's
//! two numbers.
//!
//! The split is deliberate and is the whole reason this is a library as well
//! as a binary. [`report`] never talks to a daemon, a GPU or a clock: it
//! consumes a `Vec<Event>` and computes, so the arithmetic the gate is judged
//! on is pinned by tests over synthetic journals. [`switch`] never measures
//! anything: it only issues requests. Every duration in the report was
//! recorded by the pager itself, inside the daemon, around the operation it
//! names — the bench cannot inflate or flatter a number it never takes.

pub mod http;
pub mod pressure;
pub mod report;
pub mod switch;
