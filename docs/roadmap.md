## Crust roadmap

This is a brief outline of the next stages of development that need to happen
in Crust.

1. Testing uTP

Once we have finished fully integrating uTP it needs to be tested. After
some initial internal testing we will release some small demo binary (eg. a
basic chat program or something) which makes use of uTP hole-punching so that
our users can test it on their routers.

2. Bootstrap cache

Right now the bootstrap cache doesn't get updated with new peers and doesn't
have any tests associated with it. That needs to be fixed.

3. Secure serialisation

We want to make crust encrypt all data sent along the wire using the secure
serialisation library. This has always been the intention for how crust should
work but we have never gotten round to implementing it.

4. Enhanced testing tools

We could hugely improve out testing regime with better tooling. See testing.md
for some ideas here.

