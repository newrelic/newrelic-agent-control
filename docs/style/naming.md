# Naming

## General

- Names **must** be meaningful, well thought, and discussed/agreed when there are doubts.
- Define a **ubiquitous language** and stick to it.
- Follow the [Rust API naming guidelines](https://rust-lang.github.io/api-guidelines/naming.html).

## Singular by default

Use singular names for modules and structs:

```rust
// 👍 Good:
mod agent_type;

// 👎 Bad:
mod agent_types;
```

## Struct and trait names

Struct and trait names should contain the main object they represent. Names should be meaningful even when used outside their defining module:

```rust
// 👍 Good:
struct SupervisorStopped;
trait SupervisorID;

// 👎 Bad:
struct Stopped;   // unclear outside context
trait ID;         // unclear outside context
```

## Service names

Services are named as noun forms of verbs and include the object they interact with:

```rust
// 👍 Good:
struct UserCreator;
struct AgentStarter;

// 👎 Bad:
struct CreateUser;  // verb-first
struct StartAgent;  // verb-first
struct Updater;     // no object
```

Service variable names should also include the object:

```rust
// 👍 Good:
self.supervisor_group_resolver.retrieve_group(...)

// 👎 Bad:
self.resolver.retrieve_group(...)
```

## Service method names

Service methods should use imperative verbs that self-describe their action:

```rust
user_creator.create(user);
agent_starter.start(agent);
```

## Variable names

Variables should be named after their type or the concept they represent:

```rust
// 👍 Good:
let agent_type = AgentType::default();
let started_agent_type = AgentType::default();
let super_agent_cfg = load_config();

// 👎 Bad:
let a = AgentType::new();          // too short
let agnt = AgentType::new();       // abbreviation
let agent = AgentType::new();      // ambiguous: Agent or AgentType?
let cfg = load_config();           // what config? who owns it?
```

## Loop variable names

Loop variables should be as meaningful as any other variable — a small loop can grow:

```rust
// 👍 Good:
for (agent_id, agent_cfg) in agents.iter() { ... }
for (variable_fqn, end_spec) in agent_type.variables.iter() { ... }

// 👎 Bad:
for (k, agent_cfg) in agents.iter() { ... }
for (k, v) in agent_type.variables.iter() { ... }
```

## Generic type parameter names

Generic parameters should be consistent. Prefer a word matching the trait or combination of traits over single letters:

```rust
// 👍 Good:
where Sender: Sender
where SyncedSender: Sender + Sync
```
