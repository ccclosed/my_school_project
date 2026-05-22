/// Simple round-robin multitasking scheduler for x86_64.
/// Timer IRQ triggers context switches between tasks.
///
/// SAFETY: All non-IRQ access to TASKS is guarded by disabling interrupts.
/// The IRQ handler (schedule) runs with interrupts already disabled by the CPU.
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::{arch, info, warn};

const MAX_TASKS: usize = 8;
const STACK_SIZE: u64 = 32768;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Free,
    Ready,
    Running,
}

struct Task {
    rsp: u64,
    state: TaskState,
    stack: [u8; STACK_SIZE as usize],
}

impl Task {
    const fn new() -> Self {
        Self {
            rsp: 0,
            state: TaskState::Free,
            stack: [0; STACK_SIZE as usize],
        }
    }
}

/// Task array — accessed only with interrupts disabled to prevent races with timer IRQ.
static mut TASKS: [Task; MAX_TASKS] = [const { Task::new() }; MAX_TASKS];

/// Current task index (atomic for lock-free read in IRQ context).
static CURRENT: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    unsafe {
        // Task 0 starts as Ready (will become Running on first schedule)
        TASKS[0].state = TaskState::Ready;
    }
    info!("Scheduler: idle=0, {} slots free", MAX_TASKS - 1);
}

pub fn spawn(entry: fn()) -> i32 {
    arch::disable_interrupts();
    let result = unsafe {
        for i in 1..MAX_TASKS {
            if TASKS[i].state == TaskState::Free {
                let stack_bottom = TASKS[i].stack.as_ptr() as u64;
                let stack_top = stack_bottom + STACK_SIZE;

                // Stack must be 16-byte aligned
                debug_assert!(stack_top % 16 == 0, "Stack not 16-byte aligned");

                let sp = stack_top as *mut u64;

                // Setup initial stack frame for iretq
                // After iretq, RSP will point to stack_top - 8 (misaligned for call)
                // This is correct because the entry function will be "called" via iretq
                sp.offset(-1).write(0x10u64);           // SS
                sp.offset(-2).write(stack_top - 8);     // RSP (misaligned for call)
                sp.offset(-3).write(0x200u64);          // RFLAGS (IF=1)
                sp.offset(-4).write(0x08u64);           // CS
                sp.offset(-5).write(entry as u64);      // RIP
                
                // Push general purpose registers (matching irq0 frame)
                sp.offset(-6).write(0u64);   // RAX
                sp.offset(-7).write(0u64);   // RBX
                sp.offset(-8).write(0u64);   // RCX
                sp.offset(-9).write(0u64);   // RDX
                sp.offset(-10).write(0u64);  // RBP
                sp.offset(-11).write(0u64);  // RSI
                sp.offset(-12).write(0u64);  // RDI
                sp.offset(-13).write(0u64);  // R8
                sp.offset(-14).write(0u64);  // R9
                sp.offset(-15).write(0u64);  // R10
                sp.offset(-16).write(0u64);  // R11
                sp.offset(-17).write(0u64);  // R12
                sp.offset(-18).write(0u64);  // R13
                sp.offset(-19).write(0u64);  // R14
                sp.offset(-20).write(0u64);  // R15

                TASKS[i].rsp = sp.offset(-20) as u64;
                TASKS[i].state = TaskState::Ready;
                arch::enable_interrupts();
                info!("Task {} spawned, entry=0x{:016x}", i, entry as u64);
                return i as i32;
            }
        }
        arch::enable_interrupts();
        -1
    };
    if result < 0 {
        warn!("No free task slot");
    }
    result
}

pub fn print_tasks() {
    println!("Slot State      RSP");
    arch::disable_interrupts();
    unsafe {
        for i in 0..MAX_TASKS {
            let state = TASKS[i].state;
            let rsp = TASKS[i].rsp;
            arch::enable_interrupts();
            
            if state != TaskState::Free {
                let s = match state {
                    TaskState::Free => "Free",
                    TaskState::Ready => "Ready",
                    TaskState::Running => "Run",
                };
                println!("  {}  {:4}  0x{:016x}", i, s, rsp);
            }
            
            arch::disable_interrupts();
        }
    }
    arch::enable_interrupts();
}

/// Called from the timer IRQ handler with current RSP (after push regs in irq0).
/// Interrupts are already disabled by the CPU in IRQ context.
pub fn schedule(current_rsp: u64) -> u64 {
    unsafe {
        let cur = CURRENT.load(Ordering::SeqCst);

        // Save current task state
        TASKS[cur].rsp = current_rsp;
        if TASKS[cur].state == TaskState::Running {
            TASKS[cur].state = TaskState::Ready;
        }

        // Find next ready task (round-robin)
        let mut next = (cur + 1) % MAX_TASKS;
        let mut tried = 0;
        while tried < MAX_TASKS {
            if TASKS[next].state == TaskState::Ready {
                TASKS[next].state = TaskState::Running;
                CURRENT.store(next, Ordering::SeqCst);
                return TASKS[next].rsp;
            }
            next = (next + 1) % MAX_TASKS;
            tried += 1;
        }

        // No ready task found, stay on current
        TASKS[cur].state = TaskState::Running;
        current_rsp
    }
}
