# Understanding `_module.args` in flake-parts

## What is `_module.args`?

`_module.args` is a special option in the NixOS module system that lets you **add custom arguments** that all modules can access in their function signatures.

Think of it as a "shared context" or "dependency injection" system.

## How Does It Work?

### Without `_module.args` (Regular Function Arguments)

In our current structure, we pass data explicitly:

```nix
# flake.nix
let
  rust = import ./nix/rust.nix { inherit pkgs crane rust-overlay; };
  binaryPackages = import ./nix/binary-packages.nix { 
    inherit pkgs rust;  # ← Explicit passing
  };
```

You have to manually thread data through the call chain.

### With `_module.args` (Module System)

```nix
# rust.nix - EXPORTS rust via _module.args
{
  perSystem = { ... }: {
    _module.args.rust = {
      rustToolchain = ...;
      craneLib = ...;
    };
  };
}

# binary-packages.nix - RECEIVES rust automatically
{
  perSystem = { rust, ... }: {  # ← rust appears magically!
    packages.x86_64-linux-musl = rust.craneLib.buildPackage ...;
  };
}
```

The module system automatically makes `rust` available to any module that asks for it.

## Detailed Example

Let's trace how data flows:

### Step 1: One module EXPORTS data

```nix
# nix/rust.nix
{
  perSystem = { config, pkgs, ... }: 
  let
    rustToolchain = pkgs.rust-bin.stable."1.83.0".default;
    craneLib = crane.mkLib pkgs;
  in
  {
    # This is like saying "Hey module system, make 'rust' available to everyone"
    _module.args.rust = {
      inherit rustToolchain craneLib;
    };
  };
}
```

### Step 2: Another module IMPORTS it

```nix
# nix/binary-packages.nix
{
  perSystem = { 
    config, 
    pkgs, 
    rust,   # ← This comes from _module.args.rust in rust.nix!
    ...
  }: {
    packages.default = rust.craneLib.buildPackage {
      # Use rust.craneLib here
    };
  };
}
```

### Step 3: The module system connects them

```nix
# flake.nix
flake-parts.lib.mkFlake { inherit inputs; } {
  imports = [
    ./nix/rust.nix           # Sets _module.args.rust
    ./nix/binary-packages.nix # Receives rust
  ];
}
```

The module system:
1. Evaluates all modules
2. Sees that `rust.nix` sets `_module.args.rust`
3. Makes `rust` available to ALL modules
4. Injects it into any module that has `rust` in its function signature

## Why Is It Needed?

### Without it (current approach):
```nix
# flake.nix - EXPLICIT dependency tree
let
  rust = import ./nix/rust.nix { inherit pkgs crane; };
  
  # Have to manually pass rust
  binaryPackages = import ./nix/binary-packages.nix { 
    inherit pkgs rust; 
  };
  
  # Have to manually pass rust AND binaryPackages
  distroPackages = import ./nix/distro-packages.nix {
    inherit pkgs rust;
    binaryPackages = binaryPackages.packages;
  };
  
  # Have to manually pass rust
  devshell = import ./nix/devshell.nix { 
    inherit pkgs rust; 
  };
in
{
  packages = binaryPackages.packages // distroPackages.packages;
  devShells.default = devshell;
}
```

### With `_module.args` (flake-parts):
```nix
# flake.nix - IMPLICIT dependency resolution
{
  imports = [
    ./nix/rust.nix           # exports: _module.args.rust
    ./nix/binary-packages.nix # uses: rust, exports: _module.args.binaryPackages
    ./nix/distro-packages.nix # uses: rust, binaryPackages
    ./nix/devshell.nix       # uses: rust
  ];
  # All dependencies resolved automatically!
}
```

## Built-in Arguments

The module system provides these automatically:

```nix
perSystem = {
  # Built-in arguments (always available):
  config,        # The full evaluated config
  options,       # Module options
  pkgs,          # Nixpkgs for this system
  system,        # Current system (e.g., "x86_64-linux")
  
  # Custom arguments (from _module.args):
  rust,          # If some module set _module.args.rust
  binaryPackages,# If some module set _module.args.binaryPackages
  
  ...
}: { ... }
```

## Common Pattern in flake-parts

This is a typical pattern:

```nix
# Module A: Creates something and exports it
{
  perSystem = { pkgs, ... }:
  let
    myThing = pkgs.hello;
  in
  {
    # Export for other modules
    _module.args.myThing = myThing;
    
    # Also use it locally
    packages.hello = myThing;
  };
}

# Module B: Uses the exported thing
{
  perSystem = { 
    myThing,  # ← Receives it automatically
    ...
  }: {
    packages.wrapper = pkgs.writeScriptBin "wrapper" ''
      exec ${myThing}/bin/hello
    '';
  };
}
```

## Is `_module.args` Required?

**In flake-parts: No, but it's the idiomatic way.**

You have alternatives:

### Alternative 1: Use `config` instead

```nix
# Module A: Export via packages
{
  perSystem = { ... }: {
    packages.rust-toolchain = ...;
  };
}

# Module B: Import via config
{
  perSystem = { config, ... }: {
    packages.default = buildWith config.packages.rust-toolchain;
  };
}
```

### Alternative 2: Use module options

```nix
# Module A: Define an option
{
  perSystem = { ... }: {
    options.rust.toolchain = lib.mkOption { ... };
    config.rust.toolchain = ...;
  };
}

# Module B: Read the option
{
  perSystem = { config, ... }: {
    packages.default = buildWith config.rust.toolchain;
  };
}
```

### Alternative 3: Don't use flake-parts at all!

This is what we're doing now - just use plain Nix functions:

```nix
# No module system, just functions
rust = import ./nix/rust.nix { inherit pkgs; };
binaryPackages = import ./nix/binary-packages.nix { inherit pkgs rust; };
```

## Summary

| Approach | Pros | Cons |
|----------|------|------|
| **Current (plain functions)** | Simple, explicit, easy to understand | Manual dependency threading |
| **`_module.args`** | Automatic dependency injection, less boilerplate | "Magic", need to understand module system |
| **`config` access** | Uses standard module outputs | More verbose, circular dependency risks |
| **Module options** | Type-safe, documented | Most complex, most boilerplate |

## My Take

`_module.args` is essentially **dependency injection for Nix modules**. It's:
- Convenient for complex projects
- "Magical" for simple projects
- **Not required** - your current approach works great!

For your project, I'd stick with plain functions because:
1. The dependency graph is simple (rust → everything else)
2. Explicit is clearer than implicit here
3. No need to learn module system concepts
4. Already works and is maintainable

`_module.args` shines when you have:
- Many modules depending on each other in complex ways
- Want to avoid threading arguments through many layers
- Building a larger system where the module abstraction pays off
