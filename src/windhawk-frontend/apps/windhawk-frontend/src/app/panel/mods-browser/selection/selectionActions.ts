// Which of the selected mods each of the bar's actions applies to.
//
// Pure, and the whole of the rule: the bar names a count before it runs, and it
// only ever posts requests that mean something, so both come from the same
// place.

export type SelectableMod = {
  modId: string;
  // Whether the mod has ever been compiled. One that has not cannot be turned on
  // or off - which is what its own switch says by being disabled and titled
  // "Mod needs to be compiled".
  compiled: boolean;
  disabled: boolean;
};

export type ActionTargets = {
  enable: string[];
  disable: string[];
  remove: string[];
};

/**
 * The mods each action reaches, in the order they were given.
 *
 * Enable takes the compiled mods that are off and disable the compiled ones that
 * are on: a mod already in the target state needs no request, so leaving it out
 * keeps the bar from posting a command that means nothing and from counting it
 * in what it promises. Remove takes all of them - every mod can be removed,
 * compiled or not.
 */
export function actionTargets(mods: SelectableMod[]): ActionTargets {
  const enable: string[] = [];
  const disable: string[] = [];
  const remove: string[] = [];

  for (const mod of mods) {
    if (mod.compiled) {
      if (mod.disabled) {
        enable.push(mod.modId);
      } else {
        disable.push(mod.modId);
      }
    }
    remove.push(mod.modId);
  }

  return { enable, disable, remove };
}
