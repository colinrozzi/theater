okay. i am switching over to just passing around json as much as possible.
there will only be one actor interface from now on, one that accepts both the message and the state as json.
host funtions will do whatever they need to do, and encode their message as json.
The result of the handle function will be two json objects:
- the new state
- the response message

