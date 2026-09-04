import json


print(json.dumps([
    {
        "record_id": index,
        "component": "statistics-service",
        "status": "ok",
        "observed_values": [index, index + 2, index + 4, index + 6],
        "note": "median calculation diagnostic",
    }
    for index in range(300)
]))
