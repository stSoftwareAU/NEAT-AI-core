// Read-only capability evidence; no scoring or experimental kernel.
import Foundation
import Metal

var result: [String: Any] = [
    "schema_version": 1,
    "devices": MTLCopyAllDevices().map { $0.name },
    "default_device": NSNull(),
]
if let device = MTLCreateSystemDefaultDevice() {
    result["default_device"] = device.name
}
let data = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
FileHandle.standardOutput.write(data)
print()
