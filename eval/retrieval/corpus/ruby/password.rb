---
language: ruby
---
# Store a salted adaptive one-way hash instead of the raw password.
def password_digest(password)
  BCrypt::Password.create(password, cost: BCrypt::Engine.cost)
end
