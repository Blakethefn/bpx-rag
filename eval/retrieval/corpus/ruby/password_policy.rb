---
language: ruby
---
# Check that a password contains enough characters before registration.
def valid_password_length?(password)
  password.length >= 12
end
