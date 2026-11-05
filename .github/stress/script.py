"""Writes a sermon-shaped script for Windows' voice to read (stress.yml): about 3 hours of a
preacher naming verses, reading them, paraphrasing scripture without naming it, and talking in
between. All of it is the KJV or written here, so nothing copyrighted or recorded goes in the
repository. Usage: script.py <kjv.json> <out.txt> [words]"""
import json
import random
import re
import sys

kjv = json.load(open(sys.argv[1], encoding='utf-8-sig'))
words_wanted = int(sys.argv[3]) if len(sys.argv) > 3 else 32000
rnd = random.Random(7)

# Book number (0-based, KJV order) → spoken name.
names = {0: 'Genesis', 1: 'Exodus', 4: 'Deuteronomy', 5: 'Joshua', 18: 'Psalm', 19: 'Proverbs', 22: 'Isaiah', 23: 'Jeremiah',
         39: 'Matthew', 40: 'Mark', 41: 'Luke', 42: 'John', 43: 'Acts', 44: 'Romans', 45: 'First Corinthians', 47: 'Galatians',
         48: 'Ephesians', 49: 'Philippians', 57: 'Hebrews', 58: 'James', 59: 'First Peter', 65: 'Revelation'}

talk = [
    "Church, I want you to listen to me this morning.",
    "Somebody needs to hear this today.",
    "Turn to your neighbour and tell them God is not finished with you.",
    "I know some of you came in here tired, but you will not leave the same way.",
    "Let me tell you a little story before we go on.",
    "When I was a young man, I did not understand what it meant to wait on the Lord.",
    "Can I get a witness in the house?",
    "Somebody shout hallelujah.",
    "We are going to be done soon, stay with me.",
    "This is not about religion, this is about relationship.",
    "Some of you are standing at the edge of your breakthrough.",
    "The enemy wants you to quit right before the miracle.",
    "Lift your hands if you know he has been good to you.",
    "I am not going to keep you long, but this is important.",
    "Write this down in your notes.",
    "You do not have to figure it all out today.",
    "Watch what happens next in the story.",
    "Let us keep reading, the Bible says it better than I can.",
]

paraphrases = [
    "God loved the world so much that he gave his only son, so whoever believes in him will not perish but have life forever.",
    "The Lord is my shepherd, I am not going to lack anything.",
    "I can do everything through Christ who gives me strength.",
    "All things work together for good to those who love God and are called according to his purpose.",
    "Trust in the Lord with all your heart and do not lean on your own understanding.",
    "Those who wait on the Lord will renew their strength, they will mount up with wings like eagles.",
    "Be still and know that he is God.",
    "No weapon formed against you shall prosper.",
    "Faith is the substance of things hoped for and the evidence of things not seen.",
    "Come to me all you who are weary and burdened and I will give you rest.",
    "The joy of the Lord is your strength.",
    "Weeping may last for a night but joy comes in the morning.",
    "He knows the plans he has for you, plans to prosper you and not to harm you.",
    "Greater is he that is in you than he that is in the world.",
    "If God is for us, who can be against us.",
    "Do not be anxious about anything, but in everything pray and give thanks.",
]

out, count = [], 0
def say(s):
    global count
    out.append(s)
    count += len(s.split())

while count < words_wanted:
    for _ in range(rnd.randint(2, 5)):
        say(rnd.choice(talk))
    b = rnd.choice(list(names))
    chapters = kjv[b]['chapters']
    c = rnd.randrange(len(chapters))
    verses = chapters[c]
    v = rnd.randrange(len(verses))
    n = min(len(verses) - v, rnd.randint(1, 4))
    ref = f"{names[b]} chapter {c + 1} verse {v + 1}" if n == 1 else f"{names[b]} chapter {c + 1} verses {v + 1} to {v + n}"
    say(rnd.choice(["Turn with me to", "Open your Bibles to", "Look at", "The Bible says in"]) + f" {ref}.")
    for i in range(n):
        say(re.sub(r'[{}]', '', verses[v + i]))
    for _ in range(rnd.randint(1, 3)):
        say(rnd.choice(talk))
    if rnd.random() < 0.6:
        say(rnd.choice(["You know what the word says,", "Remember,", "Hear me,"]) + " " + rnd.choice(paraphrases))

open(sys.argv[2], 'w', encoding='utf-8').write('\n'.join(out))
print(f'{count} words')
