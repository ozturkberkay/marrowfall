extends Node

# Provokes, on demand, the log lines the e2e gate greps for. Copied into a
# scratch project by the tests; never part of the game.
func _ready() -> void:
	if "--hang" in OS.get_cmdline_user_args():
		while true:
			pass
	# The orphan is never freed, so it is still in the ObjectDB at exit. The
	# call below is the one that stops this function.
	var orphan := Node.new()
	orphan.set_name("orphan")
	var nothing = null
	nothing.explode()
