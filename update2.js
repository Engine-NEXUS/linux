const fs = require('fs');
let code = fs.readFileSync('frontend/src/setup/SetupApp.tsx', 'utf8');

const step3BlockStart = code.indexOf('{/* 🔹 Step 3: Accounts 🔹 */}');
const step3BlockEnd = code.indexOf('{/* 🔹 Footer navigation 🔹 */}');

if (step3BlockStart === -1 || step3BlockEnd === -1) {
  console.log('Could not find step 3 or footer blocks');
  process.exit(1);
}

const newStep3 = fs.readFileSync('newStep3.txt', 'utf8');

const head = code.substring(0, step3BlockStart);
const tailIndex = code.indexOf('<div className=""setup-footer"">', step3BlockEnd);
const tail = code.substring(tailIndex);

const newCode = head + newStep3 + '\n          <div className="setup-footer">\n' + tail.substring('<div className="setup-footer">'.length);
fs.writeFileSync('frontend/src/setup/SetupApp.tsx', newCode);
console.log('Successfully updated SetupApp.tsx');
