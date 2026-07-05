const { resolveCollisions } = require('./overlay.js');

function assert(condition, message) {
  if (!condition) {
    throw new Error(message || "Assertion failed");
  }
}

// 1. No Collision
try {
  const input = [
    { x: 10, y: 10, width: 100, height: 50 },
    { x: 150, y: 10, width: 100, height: 50 }
  ];
  const output = resolveCollisions(input);
  assert(output.length === 2, "Should have 2 boxes");
  assert(output[0].adjustedY === 10, `Box 1 should not shift, got ${output[0]?.adjustedY}`);
  assert(output[1].adjustedY === 10, `Box 2 should not shift, got ${output[1]?.adjustedY}`);
  console.log("✓ Passed: No Collision");
} catch (e) {
  console.error("   Failed: No Collision", e.message);
  process.exitCode = 1;
}

// 2. Simple Overlap
try {
  const input = [
    { x: 10, y: 10, width: 100, height: 50 },
    { x: 15, y: 20, width: 100, height: 50 } // Overlaps
  ];
  const output = resolveCollisions(input);
  assert(output.length === 2, "Should have 2 boxes");
  assert(output[0].adjustedY === 10, `Box 1 should remain at 10, got ${output[0]?.adjustedY}`);
  assert(output[1].adjustedY === 66, `Box 2 should shift to 66, got ${output[1]?.adjustedY}`);
  console.log("✓ Passed: Simple Overlap");
} catch (e) {
  console.error("   Failed: Simple Overlap", e.message);
  process.exitCode = 1;
}

// 3. Chain Overlap
try {
  const input = [
    { x: 10, y: 10, width: 100, height: 50 },
    { x: 15, y: 20, width: 100, height: 50 }, // Overlaps box 1
    { x: 20, y: 30, width: 100, height: 50 }  // Overlaps box 1 and 2
  ];
  const output = resolveCollisions(input);
  assert(output.length === 3, "Should have 3 boxes");
  assert(output[0].adjustedY === 10, `Box 1 should remain at 10, got ${output[0]?.adjustedY}`);
  assert(output[1].adjustedY === 66, `Box 2 should shift to 66, got ${output[1]?.adjustedY}`);
  assert(output[2].adjustedY === 122, `Box 3 should shift to 122, got ${output[2]?.adjustedY}`);
  console.log("✓ Passed: Chain Overlap");
} catch (e) {
  console.error("   Failed: Chain Overlap", e.message);
  process.exitCode = 1;
}

// 4. Non-Overlapping Columns
try {
  const input = [
    { x: 10, y: 10, width: 100, height: 50 },
    { x: 120, y: 20, width: 100, height: 50 } // Side-by-side
  ];
  const output = resolveCollisions(input);
  assert(output.length === 2, "Should have 2 boxes");
  assert(output[0].adjustedY === 10, `Box 1 should remain at 10, got ${output[0]?.adjustedY}`);
  assert(output[1].adjustedY === 20, `Box 2 should remain at 20, got ${output[1]?.adjustedY}`);
  console.log("✓ Passed: Non-Overlapping Columns");
} catch (e) {
  console.error("   Failed: Non-Overlapping Columns", e.message);
  process.exitCode = 1;
}
