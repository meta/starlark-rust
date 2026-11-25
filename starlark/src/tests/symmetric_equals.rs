/*
 * Copyright 2019 The Starlark in Rust Authors.
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

//! Equality of a value with one of a type that does not know it: `1 == x` asks `x` too.

use allocative::Allocative;
use derive_more::Display;
use starlark_derive::NoSerialize;
use starlark_derive::StarlarkPagable;
use starlark_derive::starlark_module;
use starlark_derive::starlark_value;

use crate as starlark;
use crate::any::ProvidesStaticType;
use crate::assert::Assert;
use crate::environment::GlobalsBuilder;
use crate::starlark_simple_value;
use crate::values::StarlarkValue;
use crate::values::Value;
use crate::values::float::StarlarkFloat;

/// A number, equal to the `int`, the `float` and the `str` it is.
#[derive(
    Debug,
    Display,
    ProvidesStaticType,
    NoSerialize,
    Allocative,
    StarlarkPagable
)]
#[display("number({})", _0)]
struct Number(i32);
starlark_simple_value!(Number);

#[starlark_value(type = "number")]
impl<'v> StarlarkValue<'v> for Number {
    fn equals(&self, other: Value<'v>) -> starlark::Result<bool> {
        Ok(if let Some(other) = other.downcast_ref::<Number>() {
            self.0 == other.0
        } else if let Some(other) = other.unpack_i32() {
            self.0 == other
        } else if let Some(other) = other.downcast_ref::<StarlarkFloat>() {
            self.0 as f64 == other.0
        } else if let Some(other) = other.unpack_str() {
            self.0.to_string() == other
        } else {
            false
        })
    }
}

#[starlark_module]
fn numbers(builder: &mut GlobalsBuilder) {
    fn number(n: i32) -> anyhow::Result<Number> {
        Ok(Number(n))
    }
}

fn assert() -> Assert<'static> {
    let mut a = Assert::new();
    a.globals_add(numbers);
    a
}

/// Each operand on each side, through each of the instructions of `==`: a constant `int`, a
/// constant `str`, a constant of another type, and two values that are not constants. (A
/// constant whose equality is identity, such as `None` or a `str` of one character, is compared
/// by pointer and does not ask.)
#[test]
fn test_equals_asks_the_right_operand() {
    assert().pass(
        r#"
def check(x):
    assert_eq(True, x == 12)
    assert_eq(True, 12 == x)
    assert_eq(True, "12" == x)
    assert_eq(True, 12.0 == x)
    assert_eq(False, 13 == x)
    assert_eq(False, "13" == x)
    assert_eq(False, 12 != x)
    assert_eq(True, 13 != x)
    twelve = 12
    assert_eq(True, twelve == x)
    assert_eq(True, [12] == [x])
    assert_eq(True, x in [12])

check(number(12))
"#,
    );
}

/// Values of one type are not asked twice, and a value that is equal to nothing else is not.
#[test]
fn test_equals_of_one_type() {
    assert().all_true(
        r#"
number(1) == number(1)
number(1) != number(2)
1 != None
"1" != 1
"#,
    );
}
