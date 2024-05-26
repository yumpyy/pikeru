#!/usr/bin/env python
import base64, requests, json, sys

if len(sys.argv) < 3:
    quit(1)
url, file = sys.argv[1], sys.argv[2]
with open(file, "rb") as image_file:
    img = base64.b64encode(image_file.read()).decode('utf-8')
headers = {'accept': 'application/json', 'Content-Type': 'application/json'}
data = {'image': img, 'model': 'clip'}
response = requests.post(url, headers=headers, json=data)
if response.status_code > 299:
    print(response.text, file=sys.stderr)
    quit(1)
response_dict: dict = json.loads(response.text)
print(response_dict.get('caption',''))
