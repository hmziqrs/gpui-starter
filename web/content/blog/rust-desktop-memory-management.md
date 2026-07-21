---
title: "Memory management patterns for Rust desktop applications"
description: "Memory management strategies for Rust desktop apps: entity lifetimes, buffer pools, memory budgets, and leak prevention with real Rust code examples."
date: 2026-06-05
tags: [Rust, memory, deep-dive]
draft: false
---

Rust's ownership system eliminates whole classes of memory bugs at compile time. No use-after-free, no double free, no dangling pointers. But when you build a desktop application that runs for hours or days, the compiler can only protect you from so much. Memory leaks, unbounded growth, and poorly managed lifetimes are still your problem. The borrow checker prevents unsafe memory access. It does not prevent your app from consuming 2GB of RAM after six hours of use because you kept holding references to data you no longer needed.

These are the memory management patterns I rely on in Rust desktop apps. Practical techniques, not theoretical. I have hit every one of these problems in production.

## Why desktop apps are different

Server processes get restarted regularly. Deployments, crash loops, orchestration systems. A memory leak in a web service is bad, but it gets capped by restart cadence. Desktop apps run for days. Users leave them open. They sleep their laptops and wake them up. They switch between heavy workspaces, open and close panels, load and unload files.

This means two things. First, small leaks compound. A 1MB per hour leak is 24MB per day and 168MB per week. On a machine with 8GB of RAM running multiple apps, that matters. Second, peak memory matters more than average memory. Your app might sit at 50MB most of the time, but if opening a large file spikes to 800MB and that memory never gets returned, the user notices.

The ownership model gives you the tools to handle this well. You just have to use them intentionally.

## Entity lifetimes and GPUI's model

In [GPUI](/blog/building-desktop-apps-rust-gpui/), application state lives in entities. An entity is a unit of state with a defined lifetime tied to the application's context. When you create an entity, GPUI tracks it. When the context drops, the entity gets cleaned up.

```rust
#[derive(Clone)]
struct Document {
    buffer: Model<Buffer>,
    diagnostics: Vec<Diagnostic>,
}

impl Document {
    fn new(cx: &mut App) -> Self {
        let buffer = cx.new_model(|_cx| Buffer::new());
        Document {
            buffer,
            diagnostics: Vec::new(),
        }
    }
}
```

The thing to understand: `Model<T>` is a reference-counted handle. The underlying data lives as long as any handle to it exists. This is convenient, but it means you need to think about when handles get dropped.

A common mistake in GPUI apps is registering callbacks that capture entity handles and never unregistering them. The callback holds a reference, the reference keeps the entity alive, and the entity keeps its entire dependency graph in memory.

```rust
// Bad: closure captures `project` and keeps it alive forever
cx.on_release(move |_, _| {
    // This closure holds a clone of `project`
    project.read(cx).cleanup();
});

// Better: use a weak reference or scope the subscription
let weak = project.downgrade();
let subscription = cx.subscribe(&project, move |this, event, cx| {
    // Subscription is tied to the entity's lifetime
    this.handle_event(event, cx);
});
```

The subscription pattern is GPUI's answer to this problem. When you subscribe to an entity, the subscription lives as long as the entity does. When the entity drops, the subscription drops too. No manual cleanup needed.

If you are coming from a garbage-collected language, this feels backward. In JavaScript or Java, you write code and trust the GC to figure out when things can be freed. In Rust desktop apps, you design your data structures so ownership is obvious and lifetimes are narrow.

## Buffer pools for transient allocations

Desktop apps do a lot of transient allocation. Rendering frames, parsing files, processing network responses. Each of these creates temporary data that gets dropped almost immediately. The allocator handles this fine for small amounts, but when you are allocating and freeing megabytes per frame, the overhead adds up.

Buffer pooling is a straightforward solution. Instead of allocating a new buffer for each operation, you keep a pool of buffers and reuse them.

```rust
struct BufferPool {
    buffers: Vec<Vec<u8>>,
    buffer_size: usize,
}

impl BufferPool {
    fn new(buffer_size: usize) -> Self {
        BufferPool {
            buffers: Vec::new(),
            buffer_size,
        }
    }

    fn acquire(&mut self) -> Vec<u8> {
        self.buffers
            .pop()
            .unwrap_or_else(|| vec![0u8; self.buffer_size])
    }

    fn release(&mut self, mut buffer: Vec<u8>) {
        buffer.clear();
        self.buffers.push(buffer);
    }
}
```

This pattern shows up everywhere in high-performance Rust code. The `Vec` is allocated once, cleared between uses, and returned to the pool. No allocator call per frame.

The tradeoff is that you hold memory even when you are not using it. For a desktop app, that is usually fine. A pool of 10 buffers at 64KB each is 640KB. That is nothing. But it means your peak memory stays stable instead of spiking every time the user scrolls through a large file.

I use this for text rendering buffers, image decode buffers, and network read buffers. Any place where you allocate, fill, process, and drop in a tight loop.

## Memory budgets per subsystem

Instead of treating memory as a global free-for-all, give each subsystem a budget and enforce it.

```rust
struct MemoryBudget {
    limit: usize,
    used: AtomicUsize,
}

impl MemoryBudget {
    fn new(limit: usize) -> Self {
        MemoryBudget {
            limit,
            used: AtomicUsize::new(0),
        }
    }

    fn allocate(&self, size: usize) -> Result<BudgetAllocation, BudgetExceeded> {
        loop {
            let current = self.used.load(Ordering::Relaxed);
            if current + size > self.limit {
                return Err(BudgetExceeded {
                    requested: size,
                    available: self.limit.saturating_sub(current),
                });
            }
            if self.used.compare_exchange_weak(
                current,
                current + size,
                Ordering::SeqCst,
                Ordering::Relaxed,
            ).is_ok() {
                return Ok(BudgetAllocation { size });
            }
        }
    }
}
```

A memory budget forces you to make decisions about what stays and what goes. When the document buffer subsystem hits its limit, you evict old undo history or release cached syntax trees. When the image cache hits its limit, you drop the least recently used images.

Without budgets, every subsystem grows independently. The file watcher keeps more state, the autocomplete keeps more cache entries, the syntax highlighter keeps more parsed trees. Six months in, your app uses 3x the memory it did at launch and nobody knows why.

With budgets, growth is intentional. You decide that syntax trees get 50MB, image cache gets 100MB, undo history gets 20MB. When something grows, something else shrinks. The total stays bounded.

This also makes debugging easier. When a user reports high memory usage, you can log each subsystem's current usage against its budget and immediately see which one is the problem.

## Leak prevention

Rust does not have a garbage collector, so memory leaks are less common than in GC languages. But they happen. The borrow checker prevents unsafe access, not logical leaks.

The leak patterns I see most often in Rust desktop apps:

1. Cyclic references. `Rc<T>` and `Arc<T>` form cycles when two structs hold references to each other. The reference count never hits zero, so the memory is never freed. Use `Weak<T>` for back-references.

```rust
struct TreeNode {
    value: String,
    children: Vec<Rc<RefCell<TreeNode>>>,
    parent: Weak<RefCell<TreeNode>>,  // Weak breaks the cycle
}
```

2. Global registries that never get cleared. You register a handler in a static or global structure. The handler captures state. The registry grows forever. This is especially common in plugin systems and event buses.

```rust
// Problematic: handlers never removed
static HANDLERS: Lazy<Mutex<Vec<Box<dyn Fn(&Event)>>>> = Lazy::new(Default::default);

fn register(handler: Box<dyn Fn(&Event)>) {
    HANDLERS.lock().unwrap().push(handler);
}
```

The fix is to return a registration handle that removes the handler when dropped, similar to how GPUI subscriptions work.

3. Thread-local storage in long-running threads. Thread-local data lives for the lifetime of the thread. If your app has a thread pool with workers that run for days, and each task adds data to thread-local storage without cleaning up, you have a slow leak.

The rule I follow: if something can grow, it needs a bound. If it has no natural bound, I add an explicit one. Vecs get capacity limits, caches get eviction policies, and event listeners get lifetimes.

## Profiling before optimizing

Before implementing any of these patterns, profile your app. Use Instruments on macOS or `heaptrack` on Linux. Look at what is actually consuming memory. I have spent hours implementing buffer pools only to discover that 90% of the memory was going to cached images that I forgot to evict.

The workflow I recommend:

1. Profile the app at idle after launch. Note the baseline.
2. Perform a typical user session: open files, navigate around, close things.
3. Return to the same state as step 1.
4. Profile again.
5. The difference between step 4 and step 1 is your retained memory. That is what you need to investigate.

If the numbers match, you are in good shape. If step 4 shows significantly more memory than step 1, something is being retained that should not be. The heap profiler will tell you what.

## Summary

Memory management in Rust desktop apps comes down to making lifetimes obvious and keeping growth bounded. The compiler handles safety. You handle the design decisions that keep the process lean over days of use.

Entity systems like GPUI's tie state to application context so cleanup happens automatically. Buffer pools cut allocator churn on hot paths. Memory budgets stop subsystems from quietly growing without limit. And knowing the common leak patterns helps you avoid the traps that make desktop apps feel heavy after a few hours.

If you are building a desktop application with Rust and GPUI, check out [gpui-starter](/docs/getting-started/) for a production-ready boilerplate with these patterns built in. The project is [open source on GitHub](https://github.com/freeoxide/gpui-starter) and includes entity management, [persistence with SQLite](/blog/building-data-analysis-tool-rust/), and more.
