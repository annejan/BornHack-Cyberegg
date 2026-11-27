#import "lib/booklet.typ": *
#import "lib/data.typ": *
#show: booklet

#cover()

#chapter-plain("Contents", outlined: false)
#align(center, sprite("hero", "happy", k: 3))
#outline(title: none, target: heading.where(level: 1))

#include "chapters/00-meet-the-pets.typ"
#include "chapters/01-legend.typ"
#include "chapters/02-getting-started.typ"
#include "chapters/03-reading-your-pet.typ"
#include "chapters/04-the-nine-needs.typ"
#include "chapters/05-care-actions.typ"
#include "chapters/06-mood-and-health.typ"
#include "chapters/07-conditions.typ"
#include "chapters/08-off-time.typ"
#include "chapters/09-money-and-hax.typ"
#include "chapters/10-hard-mode.typ"
#include "chapters/11-friends-battles-stations.typ"
#include "chapters/12-unicorn-realm.typ"
#include "chapters/13-secrets.typ"
#include "chapters/14-for-hackers.typ"
#include "chapters/15-index.typ"

#finish()
