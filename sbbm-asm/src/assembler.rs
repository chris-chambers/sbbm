// Copyright 2015, Christopher Chambers
// Distributed under the GNU GPL v3. See COPYING for details.

use ast::{CommandBlockOut, Cond, Op, Register, Statement};
use ast::Op::*;
use ast::Statement::*;
use commands::{
    Command, IntoTarget, Objective, PlayerOp, Selector, SelectorName,
    SelectorTeam, Target, Team, players};
use commands::Command::*;
use fab;
use hw::{Computer, MemoryRegion};
use std::boxed::FnBox;
use nbt::*;
use types::{self, Block, Extent, Interval, REL_ZERO};

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::mem;
use std::{i32, u32};

use self::AssembledItem::*;

pub type PendingFn = Box<FnBox(Extent) -> Block>;

// REVIEW: AssembledItem is now used by fab, so maybe it should be renamed and
// put somewhere more general.  Seems like fab should not be use'ing assembler.
pub enum AssembledItem {
    Label(String),
    Complete(Block),
    Pending(String, PendingFn),
    Terminal,

}

impl fmt::Debug for AssembledItem {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Label(ref label) => write!(formatter, "Label({:?})", label),
            Complete(ref block) => write!(formatter, "Complete({:?}", block),
            Pending(ref name, _) => write!(formatter, "Pending({:?}, ...)", name),
            Terminal => formatter.write_str("Terminal"),
        }
    }
}

pub struct Assembler<'c, Source : Iterator<Item=Statement>> {
    computer: &'c Computer,
    input: Source,
    track_output: bool,
    buffer: VecDeque<AssembledItem>,
    target: Target,
    selector: Selector,
    uses_memory: bool,
    uses_bitwise: bool,
    done: bool,
    unique: u32,
    pending_labels: Vec<String>,
    next_addr: i32,
    label_addr_map: HashMap<String, i32>,
    team_bit: Team,
    tgt_bit_all: Target,
    tgt_bit_one: Target,
    obj_bit_comp: Objective,
    obj_bit_num: Objective,
    obj_tmp0: Objective,
    obj_tmp1: Objective,
    obj_tmp2: Objective,
    obj_two: Objective,
    obj_min: Objective,
    obj_mem_op: Objective,
    obj_mem_addr: Objective,
    obj_mem_data: Objective,
    obj_mem_tag: Objective,
}

impl<'c, S : Iterator<Item=Statement>> Assembler<'c, S> {
    pub fn new(computer: &'c Computer, assembly: S) -> Assembler<'c, S> {
        let entity_name = "computer".to_string();
        let selector = Selector {
            name: Some(SelectorName::Is(entity_name)),
            ..Selector::entity()
        };
        let target = Target::Sel(selector.clone());
        let team_bit = "Shifters";
        Assembler {
            computer: computer,
            input: assembly,
            track_output: false,
            buffer: VecDeque::new(),
            target: target,
            selector: selector,
            uses_memory: false,
            uses_bitwise: false,
            done: false,
            unique: 0,
            pending_labels: vec!(),
            next_addr: 0,
            label_addr_map: HashMap::new(),
            team_bit: team_bit.to_string(),
            tgt_bit_all: Target::Sel(Selector {
                team: Some(SelectorTeam::On(team_bit.to_string())),
                ..Selector::entity() }),
            tgt_bit_one: Target::Sel(Selector {
                team: Some(SelectorTeam::On(team_bit.to_string())),
                count: Some(1),
                ..Selector::entity() }),
            obj_bit_comp: "BitComponent".to_string(),
            obj_bit_num: "BitNumber".to_string(),
            obj_tmp0: "t0".to_string(),
            obj_tmp1: "t1".to_string(),
            obj_tmp2: "t2".to_string(),
            obj_two: "TWO".to_string(),
            obj_min: "MIN".to_string(),
            obj_mem_op: "MemOp".to_string(),
            obj_mem_addr: "MemAddr".to_string(),
            obj_mem_data: "MemData".to_string(),
            obj_mem_tag: "MemTag".to_string(),
        }
    }

    pub fn set_track_output(&mut self, value: bool) {
        self.track_output = value;
    }

    pub fn uses_memory(&self) -> bool {
        self.uses_memory
    }

    fn assemble(&mut self, stmt: Statement) {
        match stmt {
            LabelStmt(label) => { self.emit(Label(label)); }
            Instr(conds, op) => { self.assemble_instr(conds, op); }
        }
    }

    fn assemble_instr(&mut self, conds: Vec<Cond>, op: Op) {
        use commands::PlayerOp as PlOp;

        match op {
            LdrRR(dst, src) => self.emit_ldr_rr(conds, dst, src),
            StrRR(src, dst) => self.emit_str_rr(conds, src, dst),
            AddRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Add, &src),
            AddRI(dst, imm) => self.emit_radd(&conds, &dst, imm),
            AddXR(tgt, obj, src, success) =>
                self.emit_xr(&conds, &tgt, &obj, PlOp::Add, &src, &success),
            SubRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Sub, &src),
            SubRI(dst, imm) => self.emit_rsub(&conds, &dst, imm),
            SubXR(tgt, obj, src, success) =>
                self.emit_xr(&conds, &tgt, &obj, PlOp::Sub, &src, &success),
            AndRR(dst, src) => self.emit_and_rr(&conds, &dst, &src),
            OrrRR(dst, src) => self.emit_orr_rr(&conds, &dst, &src),
            EorRR(dst, src) => self.emit_eor_rr(&conds, &dst, &src),
            AsrRR(dst, src) => self.emit_asr_rr(&conds, &dst, &src),
            LsrRR(dst, src) => self.emit_lsr_rr(&conds, &dst, &src),
            LslRR(dst, src) => self.emit_lsl_rr(&conds, &dst, &src),
            MovRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Asn, &src),
            MovRI(dst, imm) => self.emit_rset(&conds, &dst, imm),
            MovRX(dst, tgt, obj) =>
                self.emit_rx(&conds, &dst, PlOp::Asn, &tgt, &obj),
            MovXR(tgt, obj, src, success) =>
                self.emit_xr(&conds, &tgt, &obj, PlOp::Asn, &src, &success),
            MulRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Mul, &src),
            SdivRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Div, &src),
            UdivRR(dst, src) => self.emit_udiv(conds, dst, src),
            SremRR(dst, src) => self.emit_rr(&conds, &dst, PlOp::Rem, &src),
            UremRR(dst, src) => self.emit_urem(conds, dst, src),
            Srng(dst, tst, min, max) => self.emit_srng(conds, dst, tst, min, max),
            Urng(dst, tst, min, max) => self.emit_urng(conds, dst, tst, min, max),
            BrL(label) => self.emit_br_l(conds, label),
            BrR(reg) => self.emit_br_r(conds, reg),
            BrLnkL(label) => self.emit_br_lnk_l(conds, label),
            BrLnkR(reg) => self.emit_br_lnk_r(conds, reg),
            Halt => self.emit(Terminal),
            RawCmd(outs, cmd) => {
                let mut block = make_cmd_block(
                    self.selector.clone(), conds, Raw(cmd), self.track_output);
                self.add_command_stats(
                    &mut block, make_command_stats(self.target.clone(), outs));
                self.emit(Complete(block));
            }
            _ => panic!("not implemented: {:?}", op)
        }
    }

    fn get_label_addr(&mut self, label: &str) -> i32 {
        if let Some(addr) = self.label_addr_map.get(label) {
            return *addr
        }

        // Skip zero as an address.
        self.next_addr += 1;
        self.label_addr_map.insert(label.to_string(), self.next_addr);
        self.next_addr
    }

    fn coalesce_label_addrs(&mut self, labels: &Vec<String>) {
        if labels.is_empty() {
            return;
        }

        let mut existing = None;
        for label in labels {
            if let Some(addr) = self.label_addr_map.get(label) {
                existing = Some(*addr);
                break;
            }
        }

        let existing = existing.unwrap_or_else(
            || self.get_label_addr(&labels[0][..]));

        for label in labels {
            if !self.label_addr_map.contains_key(label) {
                self.label_addr_map.insert(label.clone(), existing);
            }
        }
    }

    fn emit(&mut self, item: AssembledItem) {
        match item {
            Label(label) => {
                // Queue labels so they can be processed all at once, so extra
                // power-off blocks are elided.
                self.pending_labels.push(label);
            }
            _ => {
                // Flush pending labels
                if !self.pending_labels.is_empty() {
                    let first_label = self.pending_labels[0].clone();

                    // FIXME: Use drain when it is no longer unstable.
                    let mut labels = vec![];
                    mem::swap(&mut labels, &mut self.pending_labels);
                    self.coalesce_label_addrs(&labels);
                    for label in labels.into_iter() {
                        self.buffer.push_back(Label(label));
                    }

                    self.buffer.push_back(
                        fab::power_off(first_label, self.track_output));
                }

                self.buffer.push_back(item);
            }
        };
    }

    fn gen_unique_int(&mut self) -> u32 {
        let value = self.unique;
        self.unique += 1;
        value
    }

    fn gen_unique_label(&mut self, prefix: &str) -> String {
        format!("{}{}", prefix, self.gen_unique_int())
    }

    fn make_op_cmd_rr(
        &self, lhs: Register, op: PlayerOp, rhs: Register) -> Command
    {
        players::op(
            self.target.clone(), reg_name(lhs), op,
            self.target.clone(), reg_name(rhs))
    }

    fn make_op_cmd_rx(
        &self, lhs: Register, op: PlayerOp, rtgt: Target, robj: Objective)
        -> Command
    {
        players::op(self.target.clone(), reg_name(lhs), op, rtgt, robj)
    }

    fn make_op_cmd_xr(
        &self, ltgt: Target, lobj: Objective, op: PlayerOp, rhs: Register)
        -> Command
    {
        players::op(ltgt, lobj, op, self.target.clone(), reg_name(rhs))
    }

    fn emit_rr(
        &mut self, conds: &Vec<Cond>, dst: &Register, op: PlayerOp,
        src: &Register)
    {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            self.make_op_cmd_rr(dst.clone(), op, src.clone()),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_xr(
        &mut self, conds: &Vec<Cond>, tgt: &Target, obj: &Objective,
        op: PlayerOp, src: &Register, success: &Register)
    {
        let mut block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            self.make_op_cmd_xr(tgt.clone(), obj.clone(), op, src.clone()),
            self.track_output);
        self.add_success_count(&mut block, success.clone());
        self.emit(Complete(block));
    }

    fn emit_rx(
        &mut self, conds: &Vec<Cond>, dst: &Register, op: PlayerOp,
        tgt: &Target, obj: &Objective)
    {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            self.make_op_cmd_rx(dst.clone(), op, tgt.clone(), obj.clone()),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_rset(&mut self, conds: &Vec<Cond>, dst: &Register, value: i32) {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            players::set(self.target.clone(), reg_name(dst.clone()), value, None),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_radd(&mut self, conds: &Vec<Cond>, dst: &Register, count: i32) {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            players::add(self.target.clone(), reg_name(dst.clone()), count, None),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_rsub(&mut self, conds: &Vec<Cond>, dst: &Register, count: i32) {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            players::remove(self.target.clone(), reg_name(dst.clone()), count, None),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_xset(
        &mut self, conds: &Vec<Cond>, tgt: &Target, obj: &Objective, value: i32)
    {
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            players::set(tgt.clone(), obj.clone(), value, None),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_power_label(&mut self, conds: Vec<Cond>, label: String) {
        let selector = self.selector.clone();
        let track_output = self.track_output;
        self.emit(Pending(label, Box::new(move |extent| {
            match extent {
                Extent::Empty => {
                    panic!("oh no!");
                }
                Extent::MinMax(min, max) => {
                    make_cmd_block(
                        selector, conds,
                        Fill(
                            min.as_abs(), max.as_abs(),
                            "minecraft:redstone_block".to_string(),
                            None, None, None),
                        track_output)
                }
            }
        })));
    }

    // REVIEW: This can be a free function, or maybe attached to MemoryRegion as
    // a local extension trait.
    fn mem_conds(
        &self, conds: &Vec<Cond>, region: &MemoryRegion, addr: &Register)
        -> Vec<Cond>
    {
        let mut c = conds.clone();
        let end = region.start + region.size;
        // FIXME: Either handle addresses up to 4GiB (probably not needed)
        // or emit an error/warning some better way than panicking.
        if end > i32::MAX as u32 {
            panic!("Memory addresses greater than 2GiB are not supported.");
        }
        c.push(Cond::bounded(addr.clone(), region.start as i32, end as i32));
        c
    }

    fn emit_mem_tag(&mut self, conds: &Vec<Cond>, addr: &Register, id: u32) {
        // FIXME: Awkward cloning.
        let obj_mem_tag = self.obj_mem_tag.clone();

        for region in self.computer.memory.iter() {
            let region_conds = self.mem_conds(&conds, &region, &addr);
            let tgt = fab::mem_selector(region).into_target();
            self.emit_xset(&region_conds, &tgt, &obj_mem_tag, id as i32);
        }
    }

    fn emit_power_mem(&mut self, conds: &Vec<Cond>, addr: &Register) {
        for region in self.computer.memory.iter() {
            let region_conds = self.mem_conds(&conds, &region, &addr);
            let label = fab::mem_label(region);
            self.emit_power_label(region_conds.clone(), label);
        }
    }

    fn mem_tagged(&self, id: u32) -> Target {
        let tag_obj = self.obj_mem_tag.clone();
        Target::Sel(Selector {
            scores: {
                let mut s = HashMap::new();
                s.insert(tag_obj, Interval::Bounded(id as i32, id as i32));
                s },
            ..Selector::entity() })
    }

    fn emit_ldr_rr(&mut self, conds: Vec<Cond>, dst: Register, src: Register) {
        self.uses_memory = true;

        let ldr_id = self.gen_unique_int();
        self.emit_mem_tag(&conds, &src, ldr_id);
        let tagged = self.mem_tagged(ldr_id);

        // mov tagged, MemOp, 0
        // FIXME: Needing to clone self.obj_mem_op is pretty clunky.  But the
        // Assembler object will need to be internally decomposed to get rid of
        // the awkwardness.  This can come as part of a larger reorg.
        let obj_mem_op = self.obj_mem_op.clone();
        self.emit_xset(&conds, &tagged, &obj_mem_op, 0);

        // mov tagged, MemAddr, src
        // FIXME: Pass t0 for the success register to ignore the success count.
        // It would be nice to eventually handle the aux outs more generically.
        let t0 = Register::Spec(self.obj_tmp0.clone());
        // FIXME: Awkward cloning.
        let obj_mem_addr = self.obj_mem_addr.clone();
        self.emit_xr(&conds, &tagged, &obj_mem_addr, PlayerOp::Asn, &src, &t0);

        self.emit_power_mem(&conds, &src);

        // REVIEW: It would be nice to coalesce terminals here.  ldr just needs
        // a one tick delay to allow the memory controller time to produce the
        // value.
        let cont_label = self.gen_unique_label("ldr_cont_");
        self.emit_power_label(conds.clone(), cont_label.clone());
        self.emit(Terminal);
        self.emit(Label(cont_label));

        // mov dst, tagged, MemData
        // FIXME: Awkward cloning.
        let obj_mem_data = self.obj_mem_data.clone();
        self.emit_rx(&conds, &dst, PlayerOp::Asn, &tagged, &obj_mem_data);
    }

    fn emit_str_rr(&mut self, conds: Vec<Cond>, src: Register, dst: Register) {
        self.uses_memory = true;

        let str_id = self.gen_unique_int();
        self.emit_mem_tag(&conds, &dst, str_id);
        let tagged = self.mem_tagged(str_id);

        // mov tagged, MemOp, 1
        // FIXME: Awkward cloning.
        let obj_mem_op = self.obj_mem_op.clone();
        self.emit_xset(&conds, &tagged, &obj_mem_op, 1);

        // mov tagged, MemAddr, dst
        // FIXME: Pass t0 for the success register to ignore the success count.
        // It would be nice to eventually handle the aux outs more generically.
        let t0 = Register::Spec(self.obj_tmp0.clone());
        // FIXME: Awkward cloning.
        let obj_mem_addr = self.obj_mem_addr.clone();
        self.emit_xr(&conds, &tagged, &obj_mem_addr, PlayerOp::Asn, &dst, &t0);

        // mov tagged, MemData, src
        // FIXME: Pass t0 for the success register to ignore the success count.
        // It would be nice to eventually handle the aux outs more generically.
        let t0 = Register::Spec(self.obj_tmp0.clone());
        // FIXME: Awkward cloning.
        let obj_mem_data = self.obj_mem_data.clone();
        self.emit_xr(&conds, &tagged, &obj_mem_data, PlayerOp::Asn, &src, &t0);

        self.emit_power_mem(&conds, &dst);

        // REVIEW: It would be nice to coalesce terminals here.  str just needs
        // a one tick delay to allow the memory controller time to produce the
        // value.
        let cont_label = self.gen_unique_label("str_cont_");
        self.emit_power_label(conds.clone(), cont_label.clone());
        self.emit(Terminal);
        self.emit(Label(cont_label));
    }

    fn emit_and_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        let t0_obj = self.obj_tmp0.clone();
        let t1_obj = self.obj_tmp1.clone();

        self.expand_bits(conds.clone(), dst.clone(), t0_obj.clone());
        self.expand_bits(conds.clone(), src.clone(), t1_obj.clone());
        // 'and' the bits together.
        self.bit_vec_op(conds.clone(), t0_obj.clone(), PlayerOp::Mul, t1_obj);
        self.accum_bits(conds.clone(), dst.clone(), t0_obj);
    }

    fn emit_orr_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        let t0_obj = self.obj_tmp0.clone();
        let t1_obj = self.obj_tmp1.clone();

        self.expand_bits(conds.clone(), dst.clone(), t0_obj.clone());
        self.expand_bits(conds.clone(), src.clone(), t1_obj.clone());
        // 'orr' the bits together.
        self.bit_vec_op(conds.clone(), t0_obj.clone(), PlayerOp::Max, t1_obj);
        self.accum_bits(conds.clone(), dst.clone(), t0_obj);
    }

    fn emit_eor_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        let t0_obj = self.obj_tmp0.clone();
        let t1_obj = self.obj_tmp1.clone();

        self.expand_bits(conds.clone(), dst.clone(), t0_obj.clone());
        self.expand_bits(conds.clone(), src.clone(), t1_obj.clone());
        // 'eor' the bits together.
        self.bit_vec_op(conds.clone(), t0_obj.clone(), PlayerOp::Add, t1_obj);
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(), players::rem_op(
                self.tgt_bit_all.clone(), t0_obj.clone(),
                self.target.clone(), self.obj_two.clone()),
            self.track_output);
        self.emit(Complete(block));
        self.accum_bits(conds.clone(), dst.clone(), t0_obj);
    }

    fn emit_asr_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        self.raw_shift_right(conds.clone(), dst.clone(), src.clone());

        let t0_obj = self.obj_tmp0.clone();
        let lt_zero_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(dst.clone(), 0));
            c };
        let t0 = Register::Spec(t0_obj.clone());

        let sign_bits_tgt = Target::Sel(Selector {
            team: Some(SelectorTeam::On(self.team_bit.clone())),
            scores: {
                let mut s = HashMap::new();
                s.insert(t0_obj, Interval::Min(-1));
                s },
            ..Selector::entity()
        });
        // if dst < 0 execute-in-bitwise-entities: computer t0 += entity BitComponent
        let block = make_cmd_block(
            self.selector.clone(), lt_zero_conds, Execute(
                sign_bits_tgt, REL_ZERO,
                Box::new(self.make_op_cmd_rx(
                    t0.clone(), PlayerOp::Add,
                    self.tgt_bit_one.clone(), self.obj_bit_comp.clone()))),
            self.track_output);
        self.emit(Complete(block));

        // copy computer t0 to dst
        self.emit_rr(&conds, dst, PlayerOp::Asn, &t0);
    }

    fn emit_lsr_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        self.raw_shift_right(conds.clone(), dst.clone(), src.clone());

        let tmp0 = self.obj_tmp0.clone();
        let mut lt_zero_conds = conds.clone();
        lt_zero_conds.push(Cond::lt(dst.clone(), 0));
        let t0 = Register::Spec(tmp0.clone());

        let high_bit_tgt = Target::Sel(Selector {
            team: Some(SelectorTeam::On(self.team_bit.clone())),
            count: Some(1),
            scores: {
                let mut s = HashMap::new();
                s.insert(tmp0, Interval::Bounded(-1, -1));
                s },
            ..Selector::entity()
        });
        // if dst < 0 computer t0 += entity[high-bit] BitComponent
        let block = make_cmd_block(
            self.selector.clone(), lt_zero_conds, self.make_op_cmd_rx(
                t0.clone(), PlayerOp::Add,
                high_bit_tgt, self.obj_bit_comp.clone()),
            self.track_output);
        self.emit(Complete(block));

        // copy computer t0 to dst
        self.emit_rr(&conds, dst, PlayerOp::Asn, &t0);
    }

    fn emit_lsl_rr(&mut self, conds: &Vec<Cond>, dst: &Register, src: &Register) {
        self.uses_bitwise = true;

        self.activate_bitwise_entities(conds.clone(), src.clone());

        let tmp0 = self.obj_tmp0.clone();
        let two_reg = Register::Spec(self.obj_two.clone());

        let active_bit_tgt = Target::Sel(Selector {
            team: Some(SelectorTeam::On(self.team_bit.clone())),
            scores: {
                let mut s = HashMap::new();
                s.insert(tmp0, Interval::Min(0));
                s },
            ..Selector::entity()
        });
        // execute-in-bitwise-entities: dst *= TWO
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(), Execute(
                active_bit_tgt, REL_ZERO,
                Box::new(self.make_op_cmd_rr(
                    dst.clone(), PlayerOp::Mul, two_reg))),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_srng(
        &mut self, conds: Vec<Cond>, dst: Register, test: Register,
        min: Option<i32>, max: Option<i32>)
    {
        let t0 = Register::Spec(self.obj_tmp0.clone());
        let safe_test = if dst == test {
            self.emit_rr(&conds, &t0, PlayerOp::Asn, &test);
            t0
        } else {
            test
        };

        let one_conds = {
            let mut c = conds.clone();
            if let Some(interval) = Interval::new(min, max) {
                c.push(Cond::new(safe_test, interval));
            } else {
                // TODO: Issue a warning.
            }
            c };

        self.emit_rset(&conds, &dst, 0);
        self.emit_rset(&one_conds, &dst, 1);
    }

    fn emit_urng(
        &mut self, conds: Vec<Cond>, dst: Register, test: Register,
        min: Option<u32>, max: Option<u32>)
    {
        let min = min.unwrap_or(u32::MIN);
        let max = max.unwrap_or(u32::MAX);

        if min <= (i32::MAX as u32) && max <= (i32::MAX as u32) {
            // If min and max are in the range of [0, i32::MAX], emit an
            // ordinary srng.
            let min = Some(min as i32);
            let max = if max == (i32::MAX as u32) { None } else { Some(max as i32) };
            self.emit_srng(conds, dst, test, min, max);
        } else if min > (i32::MAX as u32) && max > (i32::MAX as u32) {
            // If min and max are both greater than i32::MAX, flip them to
            // negative and emit an ordinary (negative) srng.

            let min = min as i32;
            let min = if min == i32::MIN { None } else { Some(min) };
            let max = Some(max as i32);
            self.emit_srng(conds, dst, test, min, max);
        } else {
            // All other ranges (except invalid ones where min > max, which are
            // undefined behavior) require two signed ranges.
            let t0 = Register::Spec(self.obj_tmp0.clone());
            let safe_test = if dst == test {
                self.emit_rr(&conds, &t0, PlayerOp::Asn, &dst);
                t0
            } else {
                test
            };

            let a_conds = {
                let mut c = conds.clone();
                c.push(Cond::new(safe_test.clone(), Interval::Bounded(min as i32, i32::MAX)));
                c };

            let b_conds = {
                let mut c = conds.clone();
                c.push(Cond::new(safe_test, Interval::Bounded(i32::MIN, max as i32)));
                c };

            self.emit_rset(&conds, &dst, 0);
            self.emit_rset(&a_conds, &dst, 1);
            self.emit_rset(&b_conds, &dst, 1);
        }
    }

    fn emit_br_l(&mut self, conds: Vec<Cond>, label: String) {
        self.emit_br_label(conds, label, false);
    }

    fn emit_br_r(&mut self, conds: Vec<Cond>, reg: Register) {
        self.emit_br_reg(conds, reg, false);
    }

    fn emit_br_lnk_l(&mut self, conds: Vec<Cond>, label: String) {
        self.emit_br_label(conds, label, true);
    }

    fn emit_br_lnk_r(&mut self, conds: Vec<Cond>, reg: Register) {
        self.emit_br_reg(conds, reg, true);
    }

    // REVIEW: Can emit_br_label and emit_br_reg share more code?
    fn emit_br_label(&mut self, conds: Vec<Cond>, label: String, link: bool) {
        let t0 = Register::Spec(self.obj_tmp0.clone());
        self.emit_rset(&vec!(), &t0, 0);
        self.emit_rset(&conds, &t0, 1);

        let true_conds = vec!(Cond::eq(t0.clone(), 1));
        let false_conds = vec!(Cond::eq(t0, 0));

        let cont_label = self.gen_unique_label("br_cont_");
        if link {
            self.emit_branch_link(&true_conds, &cont_label);
        }

        self.emit_power_label(true_conds, label);
        self.emit_power_label(false_conds, cont_label.clone());
        self.emit(Terminal);
        self.emit(Label(cont_label));
    }

    fn emit_br_reg(&mut self, conds: Vec<Cond>, reg: Register, link: bool) {
        let t0 = Register::Spec(self.obj_tmp0.clone());
        self.emit_rset(&vec!(), &t0, 0);
        self.emit_rset(&conds, &t0, 1);

        let true_conds = vec!(Cond::eq(t0.clone(), 1));
        let false_conds = vec!(Cond::eq(t0, 0));

        let cont_label = self.gen_unique_label("br_cont_");
        if link {
            self.emit_branch_link(&true_conds, &cont_label);
        }

        // mov IndAddr, reg
        let ind_addr_reg = Register::Spec("IndAddr".to_string());
        self.emit_rr(&true_conds, &ind_addr_reg, PlayerOp::Asn, &reg);

        self.emit_power_label(true_conds, "@jump_indirect".to_string());
        self.emit_power_label(false_conds, cont_label.clone());
        self.emit(Terminal);
        self.emit(Label(cont_label));
    }

    fn emit_branch_link(&mut self, conds: &Vec<Cond>, label: &String) {
        let lr = Register::Spec("lr".to_string());
        let addr = self.get_label_addr(&label[..]);
        self.emit_rset(&conds, &lr, addr);
    }

    fn expand_bits(&mut self, conds: Vec<Cond>, reg: Register, bit_obj: Objective) {
        let tgt_all = self.tgt_bit_all.clone();
        let lt_zero_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(reg.clone(), 0));
            c
        };

        // Set all bit entities' bit_obj to the value to be expanded, reg.
        // Like this: [11, 11, 11, 11]
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            self.make_op_cmd_xr(
                tgt_all.clone(), bit_obj.clone(), PlayerOp::Asn, reg.clone()),
            self.track_output);
        self.emit(Complete(block));

        // If reg is negative, flip the sign of all temp values. This causes the
        // high bit to always end up zero, so that is handled later.
        let min_reg = Register::Spec(self.obj_min.clone());
        let block = make_cmd_block(
            self.selector.clone(), lt_zero_conds.clone(), self.make_op_cmd_xr(
                tgt_all.clone(), bit_obj.clone(), PlayerOp::Sub, min_reg),
            self.track_output);
        self.emit(Complete(block));

        // Divide all bit entities' bit_obj by their bit component.
        // Like this: [11, 11, 11, 11] / [8, 4, 2, 1] = [1, 2, 5, 11]
        let bit_comp = self.obj_bit_comp.clone();
        self.bit_vec_op(conds.clone(), bit_obj.clone(), PlayerOp::Div, bit_comp);

        // Modulo all bit entities' bit_obj by two to produce a vector of 1s
        // and 0s representing the bits of reg.
        // Like this: [1, 2, 5, 11] %= 2 = [1, 0, 1, 1]
        let block = make_cmd_block(
            self.selector.clone(), conds, players::rem_op(
                tgt_all, bit_obj.clone(),
                self.target.clone(), self.obj_two.clone()),
            self.track_output);
        self.emit(Complete(block));

        // If reg is negative, set the high bit to one.
        let tgt_high = Target::Sel(Selector {
            team: Some(SelectorTeam::On(self.team_bit.clone())),
            scores: {
                let mut s = HashMap::new();
                s.insert(self.obj_bit_num.clone(), Interval::Bounded(31, 31));
                s },
            ..Selector::entity()
        });
        let block = make_cmd_block(
            self.selector.clone(), lt_zero_conds.clone(),
            players::set(tgt_high, bit_obj, 1, None),
            self.track_output);
        self.emit(Complete(block));
    }

    fn accum_bits(&mut self, conds: Vec<Cond>, dst: Register, bit_obj: String) {
        // Multiply all bit entities' bit_obj by their bit component.
        // Like this: [1, 0, 1, 1] * [8, 4, 2, 1] = [8, 0, 2, 1]
        let bit_comp = self.obj_bit_comp.clone();
        self.bit_vec_op(conds.clone(), bit_obj.clone(), PlayerOp::Mul, bit_comp);

        // Zero the dst register.
        self.emit_rset(&conds, &dst, 0);

        // Accumulate the bit entities' bit_obj into dst.
        // Like this: dst + [8, 0, 2, 1] = 11
        let block = make_cmd_block(
            self.selector.clone(), conds, Execute(
                self.tgt_bit_all.clone(), REL_ZERO,
                Box::new(self.make_op_cmd_rx(
                    dst, PlayerOp::Add, self.tgt_bit_one.clone(), bit_obj))),
            self.track_output);
        self.emit(Complete(block));
    }

    fn bit_vec_op(&mut self, conds: Vec<Cond>, lhs: Objective, op: PlayerOp, rhs: Objective) {
        // execute @e[team=BITWISE] ~ ~ ~
        //   scoreboard players operation
        //     @e[team=BITWISE,c=1] lhs *= @e[team=BITWISE,c=1] rhs
        let block = make_cmd_block(
            self.selector.clone(), conds, Execute(
                self.tgt_bit_all.clone(), REL_ZERO,
                Box::new(players::op(
                    self.tgt_bit_one.clone(), lhs, op,
                    self.tgt_bit_one.clone(), rhs))),
            self.track_output);
        self.emit(Complete(block));
    }

    fn activate_bitwise_entities(&mut self, conds: Vec<Cond>, amount: Register) {
        let bit_num = self.obj_bit_num.clone();
        let tmp0 = self.obj_tmp0.clone();

        // SIMD copy bitwise entities' BitNumber to tmp0
        self.bit_vec_op(conds.clone(), tmp0.clone(), PlayerOp::Asn, bit_num);

        // Vector-scalar remove 32 from bitwise entities' tmp0
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(),
            players::remove(self.tgt_bit_all.clone(), tmp0.clone(), 32, None),
            self.track_output);
        self.emit(Complete(block));

        // Vector-scalar add shift amount to bitwise entities' tmp0.
        // This makes all active shifters greater than or equal to zero.
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(), self.make_op_cmd_xr(
                self.tgt_bit_all.clone(), tmp0.clone(), PlayerOp::Add, amount),
            self.track_output);
        self.emit(Complete(block));
    }

    fn raw_shift_right(&mut self, conds: Vec<Cond>, dst: Register, src: Register) {
        let tmp0 = self.obj_tmp0.clone();
        let mut lt_zero_conds = conds.clone();
        lt_zero_conds.push(Cond::lt(dst.clone(), 0));

        let t0 = Register::Spec(tmp0.clone());
        let two_reg = Register::Spec(self.obj_two.clone());
        let min_reg = Register::Spec(self.obj_min.clone());

        // Copy to t0
        self.emit_rr(&conds, &t0, PlayerOp::Asn, &dst);

        // if dst < 0, t0 -= i32::MIN
        self.emit_rr(&lt_zero_conds, &t0, PlayerOp::Sub, &min_reg);

        self.activate_bitwise_entities(conds.clone(), src);

        let active_bit_tgt = Target::Sel(Selector {
            team: Some(SelectorTeam::On(self.team_bit.clone())),
            scores: {
                let mut s = HashMap::new();
                s.insert(tmp0, Interval::Min(0));
                s },
            ..Selector::entity()
        });
        // execute-in-active-bitwise-entities: divide computer t0 by TWO
        let block = make_cmd_block(
            self.selector.clone(), conds.clone(), Execute(
                active_bit_tgt, REL_ZERO,
                Box::new(self.make_op_cmd_rr(
                    t0.clone(), PlayerOp::Div, two_reg))),
            self.track_output);
        self.emit(Complete(block));
    }

    fn emit_udiv(&mut self, conds: Vec<Cond>, dst: Register, src: Register) {
        let t0 = Register::Spec(self.obj_tmp0.clone());
        let t1 = Register::Spec(self.obj_tmp1.clone());
        let t2 = Register::Spec(self.obj_tmp2.clone());
        let min_reg = Register::Spec(self.obj_min.clone());
        let two_reg = Register::Spec(self.obj_two.clone());

        let src_pos_conds = {
            let mut c = conds.clone();
            c.push(Cond::ge(src.clone(), 0));
            c };

        let neg_pos_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(t0.clone(), 0));
            c.push(Cond::ge(src.clone(), 0));
            c };

        self.emit_rr(&conds, &t0, PlayerOp::Asn, &dst);

        // If needed, adjust dst to fit in 31 bits
        // logical shift right 1
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Add, &min_reg);
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Div, &two_reg);
        self.emit_radd(&neg_pos_conds, &dst, 1 << 30);
        // Save the current value of dst, so we can get the remainder later.
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Asn, &dst);

        // Perform the 31-bit by 31-bit division
        self.emit_rr(&src_pos_conds, &dst, PlayerOp::Div, &src);

        // If dst was adjusted to 31 bits, adjust the result to 32 bits and
        // perform the final round of division manually.
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Mul, &two_reg);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Rem, &src);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Mul, &two_reg);
        self.emit_rr(&neg_pos_conds, &t2, PlayerOp::Asn, &t0);
        self.emit_rr(&neg_pos_conds, &t2, PlayerOp::Add, &min_reg);
        self.emit_rr(&neg_pos_conds, &t2, PlayerOp::Rem, &two_reg);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Add, &t2);
        self.emit_rr(&neg_pos_conds, &t2, PlayerOp::Asn, &t1);
        self.emit_rr(&neg_pos_conds, &t2, PlayerOp::Sub, &src);
        self.emit_radd(&neg_pos_conds, &dst, 1);
        self.emit_rsub(&{
            let mut c = neg_pos_conds.clone();
            c.push(Cond::ge(t1.clone(), 0));
            c.push(Cond::lt(t2.clone(), 0));
            c }, &dst, 1);

        let src_neg_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(src.clone(), 0));
            c };

        // If src's high bit is set (negative), src dominates.
        self.emit_rset(&src_neg_conds, &dst, 0);
        //// Unless dst's (now in t0) high bit is set, and dst is larger.
        self.emit_rr(&src_neg_conds, &t1, PlayerOp::Asn, &t0);
        self.emit_rr(&src_neg_conds, &t1, PlayerOp::Sub, &src);
        self.emit_rset(&{
            let mut c = src_neg_conds.clone();
            c.push(Cond::lt(t0.clone(), 0));
            c.push(Cond::ge(t1.clone(), 0));
            c }, &dst, 1);
    }

    fn emit_urem(&mut self, conds: Vec<Cond>, dst: Register, src: Register) {
        let t0 = Register::Spec(self.obj_tmp0.clone());
        let t1 = Register::Spec(self.obj_tmp1.clone());
        let min_reg = Register::Spec(self.obj_min.clone());
        let two_reg = Register::Spec(self.obj_two.clone());

        // Save the original value of dst for later comparisons.
        self.emit_rr(&conds, &t0, PlayerOp::Asn, &dst);

        let neg_neg_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(dst.clone(), 0));
            c.push(Cond::lt(src.clone(), 0));
            c };

        // Calculate a potential remainder, but if it is negative put
        // things back like they were.  (This looks strange because dst
        // is checked twice in a row the same way, but its value
        // changes in the meantime.)
        self.emit_rr(&neg_neg_conds, &dst, PlayerOp::Sub, &src);
        self.emit_rr(&neg_neg_conds, &dst, PlayerOp::Add, &src);

        let src_pos_conds = {
            let mut c = conds.clone();
            c.push(Cond::ge(src.clone(), 0));
            c };

        let neg_pos_conds = {
            let mut c = conds.clone();
            c.push(Cond::lt(t0.clone(), 0));
            c.push(Cond::ge(src.clone(), 0));
            c };

        // If needed, adjust dst to fit in 31 bits
        // logical shift right 1
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Add, &min_reg);
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Div, &two_reg);
        self.emit_radd(&neg_pos_conds, &dst, 1 << 30);

        // Perform the 31-bit by 31-bit remainder.
        self.emit_rr(&src_pos_conds, &dst, PlayerOp::Rem, &src);

        // If dst was adjusted to 31 bits, adjust the result to 32 bits
        // and perform the final round of division manually.
        // (There is room for simplification here, I think.  The last
        // four lines may be able to eliminate their use of t1, and
        // some of the operations that go with it.)
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Mul, &two_reg);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Asn, &t0);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Add, &min_reg);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Rem, &two_reg);
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Add, &t1);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Asn, &dst);
        self.emit_rr(&neg_pos_conds, &t1, PlayerOp::Sub, &src);
        self.emit_rr(&{
            let mut c = neg_pos_conds.clone();
            c.push(Cond::lt(t1.clone(), 0));
            c.push(Cond::ge(dst.clone(), 0));
            c }, &dst, PlayerOp::Add, &src);
        self.emit_rr(&neg_pos_conds, &dst, PlayerOp::Sub, &src);
    }

    fn emit_indirect_jump_table(&mut self) {
        self.emit(Label("@jump_indirect".to_string()));

        let ind_addr_reg = Register::Spec("IndAddr".to_string());
        let mut label_addr_map = HashMap::new();
        mem::swap(&mut label_addr_map, &mut self.label_addr_map);
        let mut label_addrs: Vec<_> = label_addr_map.into_iter().collect();
        label_addrs.sort_by(|a, b| a.1.cmp(&b.1));
        for (label, addr) in label_addrs.into_iter() {
            let conds = vec![Cond::eq(ind_addr_reg.clone(), addr)];
            self.emit_power_label(conds, label);
        }

        self.emit(Terminal);
    }

    fn add_success_count(&self, block: &mut Block, reg: Register) {
        let outs = vec!((CommandBlockOut::SuccessCount, reg));
        self.add_command_stats(
            block, make_command_stats(self.target.clone(), outs));
    }

    fn add_command_stats(&self, block: &mut Block, stats: Nbt)
    {
        block.nbt.insert("CommandStats".to_string(), stats);
    }
}

fn make_cmd_block(
    selector: Selector, conds: Vec<Cond>, cmd: Command, track_output: bool)
    -> Block
{
    let cmd = if conds.is_empty() { cmd } else {
        let mut sel = selector;
        for cond in conds.into_iter() {
            sel.scores.insert(reg_name(cond.reg), cond.interval);
        }

        Execute(sel.into_target(), types::REL_ZERO, Box::new(cmd))
    };
    fab::cmd_block(cmd, track_output)
}

fn reg_name(reg: Register) -> String {
    match reg {
        Register::Gen(n) => format!("r{}", n),
        Register::Pred(n) => format!("p{}", n),
        Register::Spec(s) => s,
    }
}

fn make_command_stats(
    target: Target, outs: Vec<(CommandBlockOut, Register)>) -> Nbt
{
    let mut stats = NbtCompound::new();
    for (out, reg) in outs.into_iter() {
        stats.insert(out.selector().to_string(), Nbt::String(target.to_string()));
        stats.insert(out.objective().to_string(), Nbt::String(reg_name(reg)));
    }
    Nbt::Compound(stats)
}

impl<'c, S : Iterator<Item=Statement>> Iterator for Assembler<'c, S> {
    type Item = AssembledItem;

    fn next(&mut self) -> Option<AssembledItem> {
        while self.buffer.is_empty() {
            if let Some(stmt) = self.input.next() {
                self.assemble(stmt);
            } else if !self.done {
                self.emit(Terminal);
                self.emit_indirect_jump_table();
                self.done = true;
            } else {
                break;
            }
        }
        self.buffer.pop_front()
    }
}
