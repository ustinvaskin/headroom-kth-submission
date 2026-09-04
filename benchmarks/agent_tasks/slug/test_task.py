from implementation import slugify


assert slugify("Hello, World!") == "hello-world"
assert slugify("one___two -- three") == "one-two-three"
assert slugify("  Keep  It  Simple  ") == "keep-it-simple"
print("slug task passed")
