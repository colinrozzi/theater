[x] need to change the state-management file to a chain file
[x] need to make the architecture file not just a list of bullet points
[x] handlers - need to make the first handlers file a general explanation
[ ] make a getting-started file
[ ] README? what are we doing here? Does not really describe the project?
[x] should document all of the handlers and their configuration
[ ] then its all about updating, really not sure about the correctness of anything. on the list is:
  [ ] building-actors
  [ ] building-host-functions
  [ ] making-changes
  [ ] cli
  [ ] configuration
  [ ] api
[x] store docs are out of date, this is just dealing with the one runtime store
[ ] not really a fan of StateChain as a struct, it should just be Chain
[ ] idk
[ ] need to think about supervision more. Does not really feel like we have properly thought it through. I think this is the job of a documentation file, to which we can bring the project up to date 
[ ] events - need to implement some sort of config controlled saving/throwing away of events so that they don't accumulate too much
[ ] need to implement some sort of permissions. Right now anything is allowed to do anything. IDK.
      I am almost thinking each entity should have some public/private key, or some unique way of identifying itself. Then, we can add that identity to lists of things that are allowed to do things. Entities in this sense being actors and the external interface. For any machine maybe the external interface or some other way of identifying the user would be the most priveleged user, and would have to give out permissions to other entities?
[ ] make the theater build command go to nix and update the manifest accordingly
[ ] add nix flakes to the default projects
[ ] I really want a better events thing. I should be able to click through an actors events, view them fullscreen, access them by event id, stuff like that
[ ] rethink the message-server-host thing, the send/request are mixed in with the server functions like respond-to-request
[ ] mmmmmmmm okay i really need to figure out how I'm going to do storing events and then gettting them back from the filesystem
[ ] it would be really nice if the theater_server had an id and an interface so that we could treat it like just another actor and we could use it as a parent + stuff like that
