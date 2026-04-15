# flake-parts vs Current Structure

## Current Structure (flake-utils based)

### Pros
- ✅ Simple and straightforward
- ✅ Minimal dependencies (uses widely-known flake-utils)
- ✅ Easy to understand for Nix beginners
- ✅ Modules are just plain Nix functions
- ✅ Already working and tested

### Cons
- ❌ Manual per-system handling with `eachDefaultSystem`
- ❌ Need to manually merge module outputs (`packages = a // b`)
- ❌ Modules aren't "aware" of each other (pass data via function args)
- ❌ More boilerplate in main flake

### Current File Structure
```
flake.nix (53 lines)
  ├─ imports rust.nix
  ├─ imports binary-packages.nix (passing rust)
  ├─ imports distro-packages.nix (passing rust + binaryPackages)
  └─ imports devshell.nix (passing rust)

Each module is a function: { pkgs, rust, ... }: { ... }
```

## flake-parts Structure

### Pros
- ✅ **Standardized module system** - more idiomatic in Nix community
- ✅ **Automatic per-system handling** - no need for flake-utils
- ✅ **Better composition** - modules can import other modules
- ✅ **Shared state via `_module.args`** - modules can export data for others
- ✅ **Cleaner main flake** - just list imports
- ✅ **Extensible** - easy to add more flake-parts modules from ecosystem
- ✅ **Type safety** - better error messages with module system

### Cons
- ❌ Another dependency to learn (flake-parts)
- ❌ Slightly more complex module syntax
- ❌ Need to understand NixOS module system concepts
- ❌ More "magic" - `_module.args` passing is implicit

### flake-parts File Structure
```
flake.nix (35 lines, even simpler!)
  └─ imports: [
       ./nix/rust.nix             (defines _module.args.rust)
       ./nix/binary-packages.nix  (uses rust from _module.args)
       ./nix/distro-packages.nix  (uses rust + binaryPackages)
       ./nix/devshell.nix         (uses rust)
     ]

Each module is a flake-parts module: { ... }: { perSystem = { ... }: { ... }; }
```

## Key Differences

### How modules share data

**Current (function arguments):**
```nix
# In flake.nix
rust = import ./nix/rust.nix { inherit pkgs crane rust-overlay; };
binaryPackages = import ./nix/binary-packages.nix { inherit pkgs rust; };
```

**flake-parts (`_module.args`):**
```nix
# In rust.nix
_module.args.rust = { ... };  # Export

# In binary-packages.nix - automatically receives rust
perSystem = { rust, ... }: { ... };  # Import
```

### Main flake size

**Current:** 53 lines
```nix
outputs = { flake-utils, ... }:
  flake-utils.lib.eachDefaultSystem (system:
    let
      pkgs = import nixpkgs { inherit system overlays; };
      rust = import ./nix/rust.nix { inherit pkgs crane rust-overlay; };
      binaryPackages = import ./nix/binary-packages.nix { inherit pkgs rust; };
      distroPackages = import ./nix/distro-packages.nix { inherit pkgs rust; binaryPackages = binaryPackages.packages; };
    in
    {
      devShells.default = import ./nix/devshell.nix { inherit pkgs rust; };
      packages = binaryPackages.packages // distroPackages.packages;
    }
  );
```

**flake-parts:** 35 lines
```nix
outputs = inputs@{ flake-parts, ... }:
  flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
    imports = [
      ./nix/rust.nix
      ./nix/binary-packages.nix
      ./nix/distro-packages.nix
      ./nix/devshell.nix
    ];
  };
```

## Recommendation

### Stick with current structure if:
- ✅ You want simplicity and clarity
- ✅ Your team is new to Nix
- ✅ You don't plan to add many more features
- ✅ You prefer explicit over implicit

### Switch to flake-parts if:
- ✅ You want the most idiomatic Nix approach
- ✅ You plan to add more complex features (CI, checks, etc.)
- ✅ Your team is comfortable with NixOS modules
- ✅ You want to leverage the flake-parts ecosystem
- ✅ You want the cleanest possible main flake

## Example Migration

I've created example flake-parts files:
- `flake-parts-example.nix` - The main flake
- `nix/rust-flake-parts.nix` - Rust module
- `nix/binary-packages-flake-parts.nix` - Binary packages module
- `nix/devshell-flake-parts.nix` - Dev shell module

To test:
```bash
# Rename current flake
mv flake.nix flake-utils.nix

# Try flake-parts version
mv flake-parts-example.nix flake.nix

# Update modules to flake-parts versions
mv nix/rust.nix nix/rust-old.nix
mv nix/rust-flake-parts.nix nix/rust.nix
# ... repeat for other modules

# Test
nix flake check
```

## My Opinion

For **this project**, I'd recommend **sticking with the current structure** because:

1. It's simpler and already working
2. The project structure isn't going to get much more complex
3. The current modular approach already solves the main problem (447 → 53 line main flake)
4. flake-parts would be ~20% cleaner but adds cognitive overhead

**However**, if you were building something with:
- Multiple sub-projects
- Complex CI/CD in the flake
- Many interdependent modules
- Team very familiar with Nix

Then flake-parts would be worth it.
