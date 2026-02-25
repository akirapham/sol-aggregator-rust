import requests
import json

url = "https://api.arbiscan.io/api?module=contract&action=getabi&address=0xc873fEcbd354f5A56E00E710B90EF4201db2448d"
response = requests.get(url)
data = response.json()
if data['status'] == '1':
    abi = json.loads(data['result'])
    funcs = [f for f in abi if f.get('type') == 'function' and f.get('name') == 'exactInputSingle']
    print(json.dumps(funcs, indent=2))
else:
    print("Error:", data)
