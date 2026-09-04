import json


print(json.dumps([
    {
        "record_id": index,
        "component": "billing-service",
        "status": "ok",
        "amount_cents": -(index * 105),
        "currency": "USD",
        "note": "currency formatting diagnostic",
    }
    for index in range(300)
]))
