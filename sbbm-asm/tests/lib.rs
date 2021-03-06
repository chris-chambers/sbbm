// Copyright 2015, Christopher Chambers
// Distributed under the GNU GPL v3. See COPYING for details.

// WHEN(rust-1.0): Remove std_misc (it is here for StaticMutex and MUTEX_INIT)
#![feature(plugin, std_misc)]
#![plugin(regex_macros)]

extern crate sbbm_asm;
extern crate regex;

mod server;

use server::Server;

use std::{i32, u32};

#[test]
fn test_constant_regs() {
    let server = Server::new();

    assert_eq!(i32::MIN, server.get_computer("MIN").unwrap());
    assert_eq!(2, server.get_computer("TWO").unwrap());
    assert_eq!(0, server.get_computer("ZERO").unwrap());
}

#[test]
fn test_add() {
    let server = Server::new();

    server.run_asm("
main:
mov r0, #100
mov r1, #37
add r0, r1");

    assert_eq!(100 + 37, server.get_computer("r0").unwrap());
}

#[test]
fn test_sub() {
    let server = Server::new();

    server.run_asm("
main:
mov r0, #100
mov r1, #37
sub r0, r1");

    assert_eq!(100 - 37, server.get_computer("r0").unwrap());
}

#[test]
fn test_mul() {
    let server = Server::new();

    server.run_asm("
main:
mov r0, #100
mov r1, #37
mul r0, r1");

    assert_eq!(100 * 37, server.get_computer("r0").unwrap());
}


#[test]
fn test_sdiv() {
    let server = Server::new();

    server.run_asm("
main:
mov r0, #100
mov r1, #37
sdiv r0, r1");

    assert_eq!(100 / 37, server.get_computer("r0").unwrap());
}

#[test]
fn test_udiv() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -33, -32, -3, -2, -1,
        1, 2, 3, 32, 33, 1234567, 1234568, i32::MAX];

    for left in values.iter() {
        for right in values.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
udiv r0, r1", left, right)[..]);

            assert_eq!(
                ((*left as u32) / (*right as u32)) as i32,
                server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_srem() {
    let server = Server::new();

    server.run_asm("
main:
mov r0, #100
mov r1, #37
srem r0, r1");

    assert_eq!(100 % 37, server.get_computer("r0").unwrap());
}

#[test]
fn test_urem() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -33, -32, -3, -2,
        -1,
        1, 2, 3, 32, 33, 1234567, 1234568, i32::MAX];

    for left in values.iter() {
        for right in values.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
urem r0, r1", left, right)[..]);

            assert_eq!(
                ((*left as u32) % (*right as u32)) as i32,
                server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_and() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -1,
        0, 1, 1234567, 1234568, i32::MAX];

    for left in values.iter() {
        for right in values.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
and r0, r1", left, right)[..]);

            assert_eq!(left & right, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_orr() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -1,
        0, 1, 1234567, 1234568, i32::MAX];

    for left in values.iter() {
        for right in values.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
orr r0, r1", left, right)[..]);

            assert_eq!(left | right, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_eor() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -1,
        0, 1, 1234567, 1234568, i32::MAX];

    for left in values.iter() {
        for right in values.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
eor r0, r1", left, right)[..]);

            assert_eq!(left ^ right, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_asr() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -3, -2, -1,
        0, 1, 2, 3, 1234567, 1234568, i32::MAX];
    let amounts = [0, 1, 2, 30, 31];

    for value in values.iter() {
        for amount in amounts.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
asr r0, r1", value, amount)[..]);

            assert_eq!(value >> amount, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_lsr() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -3, -2, -1,
        0, 1, 2, 3, 1234567, 1234568, i32::MAX];
    let amounts = [0, 1, 2, 30, 31];

    for value in values.iter() {
        for amount in amounts.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
lsr r0, r1", value, amount)[..]);

            assert_eq!(
                ((*value as u32) >> amount) as i32,
                server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_lsl() {
    let server = Server::new();

    let values = [
        i32::MIN, -1234568, -1234567, -3, -2, -1,
        0, 1, 2, 3, 1234567, 1234568, i32::MAX];
    let amounts = [0, 1, 2, 30, 31];

    for value in values.iter() {
        for amount in amounts.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
lsl r0, r1", value, amount)[..]);

            assert_eq!(value << amount, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
fn test_urng() {
    let server = Server::new();

    let data = [
        ((0u32, 100u32),
         vec!((0, 1), (10, 1), (100, 1), (-1, 0), (500, 0))),
        ((u32::MAX - 2, u32::MAX),
         vec!((-4, 0), (-3, 1), (-2, 1), (-1, 1), (0, 0))),
        // TODO: The values used to test split ranges could be better.
        ((1u32 << 30, 1u32 << 31),
         vec!((-1, 0), (i32::MAX, 1), (i32::MIN, 1))),
    ];

    for &((min, max), ref in_outs) in data.iter() {
        for &(input, output) in in_outs.iter() {
            server.run_asm(&format!("
main:
mov r0, #{}
urng r0, r0, #{}, #{}", input, min, max)[..]);

            assert_eq!(output, server.get_computer("r0").unwrap());
        }
    }
}

#[test]
#[allow(overflowing_literals)]
fn test_ldr_str() {
    let server = Server::new();

    let addr_val = [
        (0x10, 1),
        (0x14, -1),
        (0x18, 0xfedcba97),
        (0x1c, 0x12345678),
        (0x20, 0),
        (0x10c, 0xedbca987),
    ];

    for &(addr, value) in addr_val.iter() {
        server.run_asm(&format!("
main:
mov r0, #{}
mov r1, #{}
str r0, [r1]", value, addr)[..]);
    }

    for &(addr, value) in addr_val.iter() {
        server.run_asm(&format!("
main:
mov r0, #{}
ldr r1, [r0]", addr)[..]);

        assert_eq!(value, server.get_computer("r1").unwrap());
    }
}

#[test]
fn test_br_r() {
    let server = Server::new();
    server.run_asm("
foo:
; prove the ordering
mov r1, #0

mov r0, #1234
b lr

main:
; prove the ordering
mov r0, #0

bl =foo
mov r1, #5678");

    assert_eq!(1234, server.get_computer("r0").unwrap());
    assert_eq!(5678, server.get_computer("r1").unwrap());
}
