/*
 * Copyright 2018 The Starlark in Rust Authors.
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     https://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

//! Detect recursion when doing `repr` or `to_json`.

use std::cell::Cell;

use crate::collections::SmallSet;
use crate::hint::unlikely;
use crate::values::Value;
use crate::values::layout::pointer::RawPointer;

/// Pop the stack on drop.
pub(crate) struct ReprStackGuard;

/// Pop the stack on drop.
pub(crate) struct JsonStackGuard;

impl Drop for ReprStackGuard {
    fn drop(&mut self) {
        REPR_STACK.with(|repr_stack| {
            let mut stack = Cell::take(repr_stack);
            let popped = stack.pop();
            debug_assert!(popped.is_some());
            repr_stack.set(stack);
        })
    }
}

impl Drop for JsonStackGuard {
    fn drop(&mut self) {
        JSON_STACK.with(|json_stack| {
            let mut stack = Cell::take(json_stack);
            let popped = stack.pop();
            debug_assert!(popped.is_some());
            json_stack.set(stack);
        })
    }
}

/// Returned when `repr` is called recursively and a cycle is detected, or when the value is nested
/// too deeply to print without running out of native stack. Both are printed the same way.
pub(crate) struct ReprCycle;

/// Why a value cannot be serialized to JSON.
pub(crate) enum JsonStackError {
    /// The value contains itself.
    Cycle,
    /// Serializing the value would run out of native stack.
    TooDeep,
}

/// Stack we refuse to walk a value into, leaving it free for the rest of the program.
///
/// Serializing goes through `erased_serde` into monomorphised `serde_json` frames, which cost
/// 10-30 KiB of native stack per level of nesting, so a value only a couple of hundred deep can
/// exhaust the stack. Without this check that aborts the process; with it we stop while there is
/// still stack left and return an error. `repr`, hashing and freezing cost far less per level, but
/// a script can nest a value as deep as it likes, so they check against the same reserve.
///
/// The reserve has to cover several levels of nesting, since it is only checked between them, plus
/// whatever the caller does with the error afterwards.
const STACK_RESERVE: usize = 256 * 1024;

/// Whether there is too little native stack left to walk another level into a value.
///
/// Not inlined, to keep measuring the stack out of the hot paths that only sometimes do it.
#[inline(never)]
fn stack_is_low() -> bool {
    // `remaining_stack` returns `None` when the platform cannot tell us, in which case there is
    // nothing to check against and we carry on.
    unlikely(stacker::remaining_stack().is_some_and(|left| left < STACK_RESERVE))
}

/// How many levels of `repr`, hashing or freezing are walked between two measurements of the stack.
///
/// Measuring at every level would cost a noticeable share of these walks, and a level of any of them
/// costs at most about 250 bytes of stack in an optimised build and 1.6 KiB in a debug one, so the
/// levels walked unchecked fit in the reserve many times over.
const STACK_CHECK_INTERVAL: u32 = 16;

/// [`stack_is_low`], measured once every [`STACK_CHECK_INTERVAL`] levels, for a walk that knows
/// how many levels deep it is.
#[inline]
pub(crate) fn stack_is_low_at_depth(depth: u32) -> bool {
    depth.is_multiple_of(STACK_CHECK_INTERVAL) && stack_is_low()
}

thread_local! {
    static STACK_CHECK_COUNTDOWN: Cell<u32> = const { Cell::new(0) };
}

/// [`stack_is_low`], measured once every [`STACK_CHECK_INTERVAL`] calls, for a walk that does not
/// know how deep it is. Each call is made at most one level deeper than the call before it, so this
/// bounds the levels walked unchecked the same way.
#[inline]
pub(crate) fn stack_is_low_sampled() -> bool {
    let countdown = STACK_CHECK_COUNTDOWN.with(|countdown| {
        let n = countdown.get();
        countdown.set(n.checked_sub(1).unwrap_or(STACK_CHECK_INTERVAL - 1));
        n
    });
    countdown == 0 && stack_is_low()
}

thread_local! {
    static REPR_STACK: Cell<SmallSet<RawPointer>> = const { Cell::new(SmallSet::new()) };
}

thread_local! {
    static JSON_STACK: Cell<SmallSet<RawPointer>> = const { Cell::new(SmallSet::new()) };
}

/// Push a value to the stack, return error if it is already on the stack, or if printing it would
/// run out of native stack.
pub(crate) fn repr_stack_push(value: Value) -> Result<ReprStackGuard, ReprCycle> {
    REPR_STACK.with(|repr_stack| {
        let mut stack = Cell::take(repr_stack);
        // The stack holds every value being printed, so its length is how deep we are.
        let res = if unlikely(
            stack_is_low_at_depth(stack.len() as u32) || !stack.insert(value.ptr_value()),
        ) {
            Err(ReprCycle)
        } else {
            Ok(ReprStackGuard)
        };
        repr_stack.set(stack);
        res
    })
}

/// Push a value to the stack, return error if it is already on the stack, or if serializing it
/// would run out of native stack.
pub(crate) fn json_stack_push(value: Value) -> Result<JsonStackGuard, JsonStackError> {
    if stack_is_low() {
        return Err(JsonStackError::TooDeep);
    }
    JSON_STACK.with(|json_stack| {
        let mut stack = Cell::take(json_stack);
        let res = if unlikely(!stack.insert(value.ptr_value())) {
            Err(JsonStackError::Cycle)
        } else {
            Ok(JsonStackGuard)
        };
        json_stack.set(stack);
        res
    })
}
