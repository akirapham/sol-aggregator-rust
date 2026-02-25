const https = require('https');
https.get("https://api.arbiscan.io/api?module=contract&action=getabi&address=0xc873fEcbd354f5A56E00E710B90EF4201db2448d", res => {
    let data = '';
    res.on('data', c => data += c);
    res.on('end', () => {
        const json = JSON.parse(data);
        if (json.status === "1") {
            const abi = JSON.parse(json.result);
            const func = abi.find(a => a.name === "exactInputSingle");
            console.log(JSON.stringify(func, null, 2));
        } else {
            console.log("Error:", json);
        }
    });
});
