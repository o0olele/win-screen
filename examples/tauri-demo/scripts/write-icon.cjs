const fs = require("fs");
const path = require("path");

const iconDir = path.join(__dirname, "..", "src-tauri", "icons");
fs.mkdirSync(iconDir, { recursive: true });

const width = 32;
const height = 32;
const xorSize = width * height * 4;
const andStride = Math.ceil(width / 32) * 4;
const andSize = andStride * height;
const dibSize = 40 + xorSize + andSize;
const icon = Buffer.alloc(6 + 16 + dibSize);

let offset = 0;
icon.writeUInt16LE(0, offset);
offset += 2;
icon.writeUInt16LE(1, offset);
offset += 2;
icon.writeUInt16LE(1, offset);
offset += 2;
icon.writeUInt8(width, offset++);
icon.writeUInt8(height, offset++);
icon.writeUInt8(0, offset++);
icon.writeUInt8(0, offset++);
icon.writeUInt16LE(1, offset);
offset += 2;
icon.writeUInt16LE(32, offset);
offset += 2;
icon.writeUInt32LE(dibSize, offset);
offset += 4;
icon.writeUInt32LE(22, offset);
offset += 4;

icon.writeUInt32LE(40, offset);
offset += 4;
icon.writeInt32LE(width, offset);
offset += 4;
icon.writeInt32LE(height * 2, offset);
offset += 4;
icon.writeUInt16LE(1, offset);
offset += 2;
icon.writeUInt16LE(32, offset);
offset += 2;
icon.writeUInt32LE(0, offset);
offset += 4;
icon.writeUInt32LE(xorSize, offset);
offset += 4;
icon.writeInt32LE(0, offset);
offset += 4;
icon.writeInt32LE(0, offset);
offset += 4;
icon.writeUInt32LE(0, offset);
offset += 4;
icon.writeUInt32LE(0, offset);
offset += 4;

for (let y = height - 1; y >= 0; y--) {
  for (let x = 0; x < width; x++) {
    const inside = x >= 6 && x < 26 && y >= 8 && y < 24;
    icon.writeUInt8(inside ? 235 : 37, offset++);
    icon.writeUInt8(inside ? 246 : 99, offset++);
    icon.writeUInt8(inside ? 255 : 235, offset++);
    icon.writeUInt8(255, offset++);
  }
}

fs.writeFileSync(path.join(iconDir, "icon.ico"), icon);
