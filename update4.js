const fs = require('fs');
let code = fs.readFileSync('frontend/src/setup/SetupApp.tsx', 'utf8');

code = code.replace(
  ""<div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '100%', padding: '0 20px', width: '100%' }}>"",
  ""<div style={{ position: 'fixed', inset: 0, background: '#E2E2E2', display: 'flex', alignItems: 'center', justifyContent: 'center', zIndex: 1000 }}>""
);

fs.writeFileSync('frontend/src/setup/SetupApp.tsx', code);
console.log('Fixed background.');
