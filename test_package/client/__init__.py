# Import the module
from __future__ import annotations

from src.pkg1 import pkg2

# Use function via module.symbol pattern
result = pkg2.some_function()
print(result)

# Use class via module.symbol pattern
instance = pkg2.SampleClass()
print(instance.method())

# Use constant via module.symbol pattern
print(pkg2.SOME_CONSTANT)

# Also test an if statement that uses the module
if pkg2.SOME_CONSTANT == "test":
    print("Test logic")
