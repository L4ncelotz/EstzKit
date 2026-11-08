import { readFileSync, readdirSync, existsSync } from "node:fs";
import { join } from "node:path";
import Ajv from "ajv";
import yaml from "js-yaml";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const pkgSchema = JSON.parse(readFileSync(join(root, "schemas", "package.schema.json"), "utf-8"));
const profileSchema = JSON.parse(readFileSync(join(root, "schemas", "profile.schema.json"), "utf-8"));

const ajv = new Ajv({ strict: false, allErrors: true });
const validatePackage = ajv.compile(pkgSchema);
const validateProfile = ajv.compile(profileSchema);

let failed = 0;

function checkDir(dir, schema, validate) {
  if (!existsSync(dir)) {
    console.log(`FAIL ${dir} missing`);
    failed++;
    return;
  }
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".yaml") || f.endsWith(".yml")).sort()) {
    const p = join(dir, file);
    const doc = yaml.load(readFileSync(p, "utf-8"));
    const ok = validate(doc);
    if (ok) {
      console.log(`OK ${p}`);
    } else {
      failed++;
      console.log(`FAIL ${p}`);
      for (const e of validate.errors ?? []) {
        console.log(`  ${e.instancePath} ${e.message}`);
      }
    }
  }
}

checkDir(join(root, "manifests", "packages"), pkgSchema, validatePackage);
checkDir(join(root, "manifests", "profiles"), profileSchema, validateProfile);

if (failed > 0) {
  console.log(`${failed} file(s) failed validation`);
  process.exit(1);
} else {
  console.log("All manifests valid");
}
