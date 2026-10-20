const fs = require('fs');
let code = fs.readFileSync('frontend/src/setup/SetupApp.tsx', 'utf8');

const regex = /\{\/\*\s*[\u2500-\u257F\u2014\u2013\u00A0-\uFFFF]*\s*Step 3: Accounts\s*[\u2500-\u257F\u2014\u2013\u00A0-\uFFFF]*\s*\*\/\}[\s\S]*?(?=\{\/\*\s*[\u2500-\u257F\u2014\u2013\u00A0-\uFFFF]*\s*Footer navigation\s*[\u2500-\u257F\u2014\u2013\u00A0-\uFFFF]*\s*\*\/\})/g;

const newStep3 = fs.readFileSync('newStep3.txt', 'utf8');

if (!regex.test(code)) {
    console.log('Regex did not match.');
    process.exit(1);
}

code = code.replace(regex, newStep3);
fs.writeFileSync('frontend/src/setup/SetupApp.tsx', code);
console.log('Successfully replaced via regex.');
