from implementation import format_cents


assert format_cents(105) == "1.05"
assert format_cents(-105) == "-1.05"
assert format_cents(-5) == "-0.05"
print("currency task passed")
