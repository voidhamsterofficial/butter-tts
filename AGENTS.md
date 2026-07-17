# AGENTS.md

Guidance for AI agents (and humans) working in this codebase. Read this before writing or modifying any code.

---

## 1. Coding Standards

We hold a high bar for code quality. Code is read far more often than it is written — optimise for the next person (or agent) who has to understand it.

### 1.1 DRY — Don't Repeat Yourself

Every piece of knowledge should have a single, authoritative representation in the codebase.

- If you find yourself copying a block of code, stop and extract it into a shared function, constant, or module.
- Duplicated logic drifts. When two copies of the same rule exist, one will eventually be updated and the other forgotten.
- This applies beyond code: duplicated config values, magic numbers, validation rules, and type definitions should all live in one place and be referenced everywhere else.
- **However** — don't abstract prematurely. Two things that look similar today but serve different concerns may be incidental duplication. Prefer a little duplication over the wrong abstraction. The test: if the two copies *must* change together, they should be one thing.

```rust
// ❌ Bad — the same discount rule lives in two places
let cart_total: f64 = items.iter().map(|item| item.price * 0.9).sum();
// ...elsewhere...
let invoice_total: f64 = lines.iter().map(|line| line.price * 0.9).sum();

// ✅ Good — one source of truth
const MEMBER_DISCOUNT_MULTIPLIER: f64 = 0.9;

fn apply_member_discount(price: f64) -> f64 {
    price * MEMBER_DISCOUNT_MULTIPLIER
}
```

### 1.2 Separation of Concerns

Each module, struct, and function should have one clear responsibility.

- **Keep layers apart.** Data access, business logic, and presentation should not be tangled together in one function or file. A function that fetches data, transforms it, *and* renders it is three functions wearing a trench coat.
- **Functions do one thing.** If you need the word "and" to describe what a function does, it probably needs splitting.
- **Modules own a domain.** Group code by what it's *about*, not by what it *is*. Prefer a `billing` module containing its own types, services, and validators over a giant shared `utils` bucket.
- **Side effects live at the edges.** Keep core logic pure and testable; push I/O (network, filesystem, database) to the boundaries of the system. Pure functions don't need `async`, mocks, or test servers.

```rust
// ❌ Bad — fetching, business rules, and formatting in one place
async fn show_user_report(user_id: &str) -> Result<(), ReportError> {
    let response = reqwest::get(format!("https://api.example.com/users/{user_id}")).await?;
    let user: User = response.json().await?;
    let is_eligible = user.purchases > 5 && !user.suspended;
    println!("<p>{}: {}</p>", user.name, is_eligible);
    Ok(())
}

// ✅ Good — each concern is separate and independently testable
async fn fetch_user(user_id: &str) -> Result<User, FetchError> { /* data access */ }

fn is_eligible_for_rewards(user: &User) -> bool {
    user.purchases > MINIMUM_PURCHASES_FOR_REWARDS && !user.suspended
}

fn render_user_report(user: &User, is_eligible: bool) -> String { /* presentation */ }
```

### 1.3 No Inline Returns

Do not return complex expressions directly. Bind the result to a well-named variable first, then return it.

- A named intermediate variable documents *what* the expression means, not just what it computes.
- It gives you somewhere to set a breakpoint or add a `dbg!`/`tracing` line when debugging.
- It keeps diffs clean when the logic later needs a guard clause or transformation before returning.

```rust
// ❌ Bad — what does this expression represent?
fn get_price(item: &Item, user: &User) -> f64 {
    item.base_price * if user.is_member { 0.9 } else { 1.0 }
        + if item.weight_kg > 20.0 { 15.0 } else { 0.0 }
}

// ✅ Good — every step is named and inspectable
fn get_price(item: &Item, user: &User) -> f64 {
    let discount_multiplier = if user.is_member { MEMBER_DISCOUNT_MULTIPLIER } else { 1.0 };
    let discounted_price = item.base_price * discount_multiplier;
    let heavy_item_surcharge = if item.weight_kg > HEAVY_ITEM_THRESHOLD_KG { HEAVY_ITEM_FEE } else { 0.0 };

    discounted_price + heavy_item_surcharge
}
```

Simple early-exit guards (`return None;`, `return Vec::new();`, `return is_valid;`) are fine — the rule targets *expressions with logic in them*, not trivial returns.

**Do not add a binding that exists only to be returned.** `let final_price = a + b; final_price` names nothing the reader didn't already know from the function's own name and return type, and clippy rejects it (`clippy::let_and_return`). Name the *steps*, then return the last expression directly. The same goes for a body that is one trivial call — write `Self::default()`, not `let detector = Self::default(); detector`.

Where this section and clippy disagree, **clippy wins**: fix the code, never reach for `#[allow(clippy::let_and_return)]` (see 1.6). The value of this rule is in naming the intermediate steps of a calculation, not in the final line.

The same rule applies to **cramming control flow onto one line**. A dense chain of combinators (`map_or_else`, nested `and_then`, a ternary-style `if` buried mid-expression) hides control flow and gives you nowhere to add a log or a second statement later. Prefer explicit `match`, `if let`/`let else`, and braced blocks:

```rust
// ❌ Bad — control flow hidden in a one-liner
let Some(user) = maybe_user else { return None };
if order.is_paid { send_receipt(&order); } else { flag_for_review(&order); }

// ✅ Good — explicit, braced, and extensible
let Some(user) = maybe_user else {
    return None;
};

if order.is_paid {
    send_receipt(&order);
} else {
    flag_for_review(&order);
}
```

### 1.4 Maximum Nesting Depth: 2 — Prefer Flat Code

Deeply nested code is hard to read, hard to test, and hides the happy path. **Two levels of nesting is the hard ceiling. Zero or one is the goal.**

- **Use guard clauses.** Handle the invalid/edge cases first and return early, so the main logic sits at the top level instead of inside an `if` pyramid.
- **Use `?`, `let else`, and early `return`** to exit on the error/empty path rather than wrapping the rest of the function in `if let Some(x) = ... { ... }`.
- **Extract nested blocks into named functions.** If a loop body or match arm is complex enough to nest further, it's complex enough to deserve its own name (see 1.5).
- **Flatten loops with iterator adapters or continue-guards.** `filter`/`map` chains, or an early `continue` inside a loop, remove a level of nesting each.

```rust
// ❌ Bad — four levels deep, the actual logic is buried
fn process_orders(orders: &[Order]) {
    for order in orders {
        if order.is_active {
            if !order.items.is_empty() {
                for item in &order.items {
                    if item.in_stock {
                        ship_item(item);
                    }
                }
            }
        }
    }
}

// ✅ Good — guard clauses and extraction keep everything flat
fn process_orders(orders: &[Order]) {
    let active_orders = orders.iter().filter(|order| order.is_active);

    for order in active_orders {
        ship_in_stock_items(order);
    }
}

fn ship_in_stock_items(order: &Order) {
    let in_stock_items = order.items.iter().filter(|item| item.in_stock);

    for item in in_stock_items {
        ship_item(item);
    }
}
```

If you find yourself needing a third level of nesting, that is a signal to restructure — extract a function, add a guard clause, or rethink the data flow. Do not just indent further.

### 1.5 Human-Readable Names for Everything

Names are the primary documentation. Every variable, function, struct, trait, module, and parameter should say what it is without needing a comment.

- **Follow Rust naming conventions:** `snake_case` for functions/variables/modules, `UpperCamelCase` for types/traits/enum variants, `SCREAMING_SNAKE_CASE` for constants and statics.
- **No abbreviations or single letters** outside of tiny, conventional scopes (`i` in a short loop index is acceptable; `usr_mgr_svc` never is).
- **Booleans read as questions or states:** `is_loading`, `has_expired`, `can_retry` — not `flag`, `check`, or `status2`.
- **Functions are verbs, values are nouns:** `calculate_invoice_total()` returns `invoice_total`.
- **Name the units and context:** `timeout_ms`, `distance_km`, `retry_count` — not `timeout`, `distance`, `n`. Better still, use newtypes or `std::time::Duration` so the type carries the unit.
- **If naming something is hard, the design is probably wrong.** A function you can't name concisely is usually doing too many things (see 1.2).

```rust
// ❌ Bad
let d = Utc::now();
let res = get_data(u.id);
fn proc(x: &[Value]) { /* ... */ }

// ✅ Good
let report_generated_at = Utc::now();
let purchase_history = fetch_purchase_history(user.id);
fn summarise_monthly_purchases(purchases: &[Purchase]) { /* ... */ }
```

### 1.6 Handle Errors Properly — No Panics, No Silencing

Rust's type system is the contract. `unwrap()` deletes the contract at runtime; suppressing warnings defers it. Both hide bugs the compiler exists to catch.

- **Never use `unwrap()` or `expect()` in production code paths.** Propagate errors with `?` and `Result`, or handle the `None`/`Err` case explicitly. `expect()` is acceptable only for invariants that are provably impossible to violate (and the message should say why), and in tests, examples, and build scripts.
- **Never use `panic!`, `todo!`, or `unimplemented!`** as error handling in shipped code. Panics are for unrecoverable programmer errors, not expected failure modes like bad input or I/O errors.
- **Model errors properly.** Define error enums (e.g. with `thiserror`) for library code; use `anyhow`-style context only at the application boundary. If an error type is awkward, that's a design signal, not a reason to `.unwrap()`.
- **Avoid `unsafe`.** If it is genuinely required, isolate it in the smallest possible scope and document the safety invariants with a `// SAFETY:` comment.
- Do not silence the compiler or clippy with `#[allow(...)]`, `as` casts that truncate, or `let _ =` on a `Result`. Fix the underlying issue instead. Code must compile without warnings and pass `cargo clippy`.
- **Clippy is the tiebreaker.** Where a rule in this document appears to conflict with a clippy lint, clippy wins and the code changes to satisfy it — this document is the thing that gets amended, not the lint that gets suppressed. If following a rule here forces you toward `#[allow(...)]`, that is a bug in the rule: fix the code, then raise the wording so the next person doesn't hit the same wall.

```rust
// ❌ Bad
fn handle_response(body: &str) -> Vec<String> {
    let data: serde_json::Value = serde_json::from_str(body).unwrap();
    data["items"].as_array().unwrap().iter()
        .map(|x| x["name"].as_str().unwrap().to_string())
        .collect()
}

// ✅ Good — typed deserialization, errors propagated
#[derive(Deserialize)]
struct ProductResponse {
    items: Vec<Product>,
}

fn handle_response(body: &str) -> Result<Vec<String>, serde_json::Error> {
    let response: ProductResponse = serde_json::from_str(body)?;
    let product_names = response.items.into_iter().map(|product| product.name).collect();
    Ok(product_names)
}
```

### 1.7 Keep It Simple — Don't Over-Complicate

Simplicity is a feature. The best solution is the most boring one that fully solves the problem. Cleverness is a cost, not an achievement.

- **Solve the problem in front of you.** Don't build for hypothetical future requirements (YAGNI). Speculative flexibility — extra config options, generic type parameters, plugin systems, abstractions "in case we need them" — is complexity paid for now for value that usually never arrives.
- **Prefer boring, obvious constructs.** A plain `if/else`, a straightforward `for` loop, or a simple function beats a dense combinator chain, a clever macro, or an exotic trait bound. If a reader has to pause and decode it, it's too clever.
- **Fewer moving parts.** Every new layer, wrapper, dependency, or design pattern must earn its place. Don't add a trait with one implementer, generics with one concrete type, `Arc<Mutex<...>>` where ownership would do, or a channel where a function call would do.
- **Don't reach for lifetimes and borrowing gymnastics prematurely.** A `.clone()` on a small struct is cheaper than an unreadable web of lifetime annotations. Optimise for clarity first, allocation count later — and only with a measurement.
- **Small functions, small files, small changes.** If a solution keeps growing, step back — you may be solving a harder problem than the one that was asked.
- **Delete code gladly.** The simplest code is the code that doesn't exist. Removing complexity is as valuable as adding features.

```rust
// ❌ Bad — clever, dense, and over-engineered for what it does
fn get_status(user: &User) -> &'static str {
    user.subscription.as_ref().map_or("none", |s| {
        if s.tier == Tier::Pro || s.expires_at > Utc::now() { "active" } else { "lapsed" }
    })
}

// ✅ Good — boring, flat, and obvious
fn get_subscription_status(user: &User) -> SubscriptionStatus {
    let Some(subscription) = &user.subscription else {
        return SubscriptionStatus::None;
    };

    let is_pro_tier = subscription.tier == Tier::Pro;
    let is_still_valid = subscription.expires_at > Utc::now();

    if is_pro_tier || is_still_valid {
        return SubscriptionStatus::Active;
    }

    SubscriptionStatus::Lapsed
}
```

The test for every change: **could a new team member read this cold and understand it in one pass?** If not, simplify.

---

## 2. Workflow: Explore Before You Act

Do not start writing code the moment you receive a task. Understanding the codebase first prevents duplicated work, broken conventions, and misplaced changes.

### 2.1 Understand the Project First

Before making any change:

1. **Read the relevant code paths.** Find the files involved in the task and read them properly — not just the function you're changing, but its callers and the module around it.
2. **Learn the existing conventions.** How does this project structure modules? Name things? Handle errors? Write tests? Match the established patterns rather than importing your own.
3. **Check whether it already exists.** Search for existing utilities, helpers, or services before writing new ones — in the codebase *and* in the crates already listed in `Cargo.toml`. Reinventing an existing helper violates DRY (1.1) at the project level.
4. **Trace, don't assume.** If you're unsure how data flows or where something is used, follow the `use` statements and references. Guessing at behaviour leads to changes that compile but break things.
5. **Make the smallest change that solves the problem.** Prefer targeted edits within existing structures over rewrites and reorganisations, unless restructuring is the actual task.
6. **Verify with the standard tools.** `cargo check`, `cargo test`, `cargo clippy`, and `cargo fmt` must all pass before a change is done.

### 2.2 Avoid Huge Bespoke Scripts

Do not solve problems by generating large, one-off scripts when smaller, standard approaches exist.

- **Prefer existing tooling.** Use `cargo` commands (`build`, `test`, `run`, `clippy`, `fmt`), the project's own binaries and examples, and any `Makefile`/`justfile` tasks before writing anything custom.
- **Prefer small, composable steps.** A few short, verifiable commands are better than one 300-line script that does everything invisibly. Small steps can be checked as you go; a monolithic script fails as a black box.
- **Don't script what should be code.** If logic is worth keeping, it belongs in the codebase — properly placed, named, typed, and tested — not in a throwaway script.
- **Don't script what should be manual.** For a one-time, three-file change, just edit the three files. Writing a script to automate a task smaller than the script itself is wasted effort and added risk.
- If a substantial script is genuinely necessary, explain why first, keep it minimal, and clean it up afterwards.

---

## 3. Quick Checklist

Before submitting any change, confirm:

- [ ] I explored the relevant code and followed existing conventions
- [ ] No logic or values are duplicated — shared code is extracted
- [ ] Each function/module has a single, clear responsibility
- [ ] No complex expressions are returned inline — the steps of a calculation are named first
- [ ] No binding that exists only to be returned (`let x = ...; x`) — clippy rejects it
- [ ] No control flow crammed onto one line — branches use explicit, braced blocks
- [ ] Nesting never exceeds 2 levels — guard clauses, `?`, and extraction keep code flat
- [ ] Every name is descriptive, unabbreviated, and follows Rust naming conventions
- [ ] No `unwrap()`/`expect()`/`panic!` in production paths — errors are typed and propagated with `?`
- [ ] No silenced warnings — `cargo check`, `cargo clippy`, `cargo fmt`, and `cargo test` all pass
- [ ] The solution is the simplest one that works — no speculative abstractions or clever tricks
- [ ] I didn't write a large bespoke script where existing tools or small steps would do
