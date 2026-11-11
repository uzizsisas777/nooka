// Lista los tipos de recurso de un PE (verificacion del hito B: VERSIONINFO=16, MANIFEST=24,
// ICON=3, GROUP_ICON=14). Uso: node scripts/pe-resources.mjs ruta.exe
import { readFile } from "node:fs/promises";

const NAMES = { 1: "CURSOR", 2: "BITMAP", 3: "ICON", 9: "GROUP_CURSOR", 14: "GROUP_ICON", 16: "VERSION", 24: "MANIFEST", 11: "GROUP_VERSION" };

const buf = await readFile(process.argv[2]);
const peOff = buf.readUInt32LE(0x3c);
if (buf.toString("ascii", peOff, peOff + 4) !== "PE\0\0") throw new Error("no es PE");
const nSections = buf.readUInt16LE(peOff + 6);
const sizeOptional = buf.readUInt16LE(peOff + 20);
const optOff = peOff + 24;
const magic = buf.readUInt16LE(optOff);
const ddOff = optOff + (magic === 0x20b ? 112 : 96);
const rsrcRva = buf.readUInt32LE(ddOff + 2 * 8);
const rsrcSize = buf.readUInt32LE(ddOff + 2 * 8 + 4);
if (!rsrcRva) {
  console.log("sin seccion .rsrc");
  process.exit(0);
}
const secOff = optOff + sizeOptional;
let rsrcFileOff = -1;
for (let i = 0; i < nSections; i++) {
  const s = secOff + i * 40;
  const va = buf.readUInt32LE(s + 12);
  const raw = buf.readUInt32LE(s + 20);
  if (va === rsrcRva) {
    rsrcFileOff = raw;
    break;
  }
}
if (rsrcFileOff < 0) throw new Error("RVA de .rsrc no casado con ninguna seccion");

const walk = (dirOff) => {
  const named = buf.readUInt16LE(rsrcFileOff + dirOff + 12);
  const ids = buf.readUInt16LE(rsrcFileOff + dirOff + 14);
  const out = [];
  for (let i = 0; i < named + ids; i++) {
    const e = rsrcFileOff + dirOff + 16 + i * 8;
    const nameOrId = buf.readUInt32LE(e);
    const sub = buf.readUInt32LE(e + 4);
    if (dirOff === 0) {
      const id = nameOrId & 0x7fffffff;
      const typeName = NAMES[id] || `tipo_${id}`;
      if (sub & 0x80000000) {
        let entries = 0;
        let versions = 0;
        const typeDir = sub & 0x7fffffff;
        const n2 = buf.readUInt16LE(rsrcFileOff + typeDir + 12);
        const i2 = buf.readUInt16LE(rsrcFileOff + typeDir + 14);
        for (let j = 0; j < n2 + i2; j++) {
          const e2 = rsrcFileOff + typeDir + 16 + j * 8;
          const sub2 = buf.readUInt32LE(e2 + 4);
          if (!(sub2 & 0x80000000)) entries++;
          else versions += buf.readUInt16LE(rsrcFileOff + (sub2 & 0x7fffffff) + 14);
        }
        out.push(`${typeName} (${id}): ${entries + versions} recurso(s)`);
      } else {
        out.push(`${typeName} (${id}): data directo`);
      }
    }
  }
  return out;
};
console.log(`${process.argv[2]}\n  ${walk(0).join("\n  ")}`);
