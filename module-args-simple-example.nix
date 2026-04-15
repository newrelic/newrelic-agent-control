# Simple example showing _module.args vs regular function passing

# ============================================================================
# APPROACH 1: Without _module.args (what we're doing now - plain functions)
# ============================================================================
let
  # Module A creates some data
  moduleA = { pkgs }: {
    data = "Hello from module A";
    tool = pkgs.hello;
  };

  # Module B needs data from A - we MANUALLY pass it
  moduleB = { pkgs, moduleAData }: {
    result = "Module B says: ${moduleAData.data}";
  };

  # In main flake - EXPLICIT wiring
  a = moduleA { inherit pkgs; };
  b = moduleB { inherit pkgs; moduleAData = a; };  # ← Manual passing
in
{
  inherit a b;
}

# ============================================================================
# APPROACH 2: With _module.args (flake-parts style)
# ============================================================================
# This would be split across multiple files, but showing inline for clarity
{
  imports = [
    # Module A - EXPORTS via _module.args
    ({ config, pkgs, ... }: {
      _module.args.moduleAData = {
        data = "Hello from module A";
        tool = pkgs.hello;
      };
    })

    # Module B - RECEIVES automatically
    ({ config, pkgs, moduleAData, ... }: {
      # moduleAData just appears! No manual passing needed
      packages.result = pkgs.writeText "result"
        "Module B says: ${moduleAData.data}";
    })
  ];
}

# ============================================================================
# How it works under the hood (simplified)
# ============================================================================
# The module system does this:
# 1. Collect all _module.args from all modules:
#    allArgs = { moduleAData = ...; anotherArg = ...; }
#
# 2. Merge with built-in args:
#    finalArgs = builtinArgs // allArgs
#    where builtinArgs = { config, pkgs, lib, ... }
#
# 3. Call each module with finalArgs:
#    module1 finalArgs
#    module2 finalArgs
#    ...

# ============================================================================
# Real-world analogy
# ============================================================================

# WITHOUT _module.args (explicit dependency injection):
makeTeam = {
  # Create chef
  chef = makeChef { };

  # Create waiter - must explicitly give them the chef
  waiter = makeWaiter { inherit chef; };

  # Create manager - must explicitly give them chef AND waiter
  manager = makeManager { inherit chef waiter; };
}

# WITH _module.args (implicit dependency injection):
makeTeam = {
  imports = [
    # Chef registers themselves
    { _module.args.chef = makeChef { }; }

    # Waiter can just ask for chef
    { waiter, ... }: { waiter = makeWaiter { inherit chef; }; }

    # Manager can ask for both
    { chef, waiter, ... }: { manager = makeManager { inherit chef waiter; }; }
  ];
  # Module system automatically wires everything up!
}
