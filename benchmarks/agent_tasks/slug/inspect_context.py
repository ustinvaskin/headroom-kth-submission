import json


print(json.dumps([
    {
        "record_id": index,
        "component": "url-service",
        "status": "ok",
        "raw_title": f"Release {index}: Hello, World!",
        "separator": "mixed punctuation",
        "note": "slug normalization diagnostic",
    }
    for index in range(300)
]))
