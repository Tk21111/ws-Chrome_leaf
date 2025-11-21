ver 2
tcp - global
ws - local

global :
tcp <-> ws
local : 
ws <-> window

process flow
edge checker ----- local_channel -----> forwarder ( json converter ) --- ws ---> chrome_ext ----  ws ----> forwarder ---- tcp ----> another_computer 
