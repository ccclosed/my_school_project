/// Simple round-robin multitasking scheduler for i686.
/// Timer IRQ triggers context switches between tasks.
///
/// SAFETY: All non-IRQ access to TASKS is guarded by disabling interrupts.
/// The IRQ handler (schedule) runs with interrupts already disabled by the CPU.
use core::sync::atomic::{AtomicUsize, Ordering};
use crate::{arch, info, warn};

const MAX_TASKS: usize = 8;
const STACK_SIZE: u32 = 16384;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TaskState {
    Free,
    Ready,
    Running,
}

struct Task {
    esp: u32,
    state: TaskState,
    stack: [u8; STACK_SIZE as usize],
}

impl Task {
    const fn new() -> Self {
        Self {
            esp: 0,
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
                let stack_bottom = TASKS[i].stack.as_ptr() as u32;
                let stack_top = stack_bottom + STACK_SIZE;

                debug_assert!(stack_top % 16 == 0, "Stack not 16-byte aligned");

                let sp = stack_top as *mut u32;

                sp.offset(-1).write(0x200u32);
                sp.offset(-2).write(0x08u32);
                sp.offset(-3).write(entry as u32);
                sp.offset(-4).write(0u32);
                sp.offset(-5).write(0u32);
                sp.offset(-6).write(0u32);
                sp.offset(-7).write(0u32);
                sp.offset(-8).write(0u32);
                sp.offset(-9).write(0u32);
                sp.offset(-10).write(0u32);
                sp.offset(-11).write(0u32);

                TASKS[i].esp = sp.offset(-11) as u32;
                TASKS[i].state = TaskState::Ready;
                arch::enable_interrupts();
                info!("Task {} spawned, entry=0x{:08x}", i, entry as u32);
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
    println!("Slot State      ESP");
    arch::disable_interrupts();
    unsafe {
        for i in 0..MAX_TASKS {
            let state = TASKS[i].state;
            let esp = TASKS[i].esp;
            arch::enable_interrupts();
            
            if state != TaskState::Free {
                let s = match state {
                    TaskState::Free => "Free",
                    TaskState::Ready => "Ready",
                    TaskState::Running => "Run",
                };
                println!("  {}  {:4}  0x{:08x}", i, s, esp);
            }
            
            arch::disable_interrupts();
        }
    }
    arch::enable_interrupts();
}

/// Called from the timer IRQ handler with current ESP (after pushad in irq0).
/// Interrupts are already disabled by the CPU in IRQ context.
pub fn schedule(current_esp: u32) -> u32 {
    unsafe {
        let cur = CURRENT.load(Ordering::SeqCst);

        // Save current task state
        TASKS[cur].esp = current_esp;
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
                return TASKS[next].esp;
            }
            next = (next + 1) % MAX_TASKS;
            tried += 1;
        }

        // No ready task found, stay on current
        TASKS[cur].state = TaskState::Running;
        current_esp
    }
}
