/* Load the real brand fonts so the render matches the site (not a serif
   fallback). Imported once from Root so every composition has them. */
import {loadFont as loadInter} from '@remotion/google-fonts/Inter';
import {loadFont as loadSpaceGrotesk} from '@remotion/google-fonts/SpaceGrotesk';
import {loadFont as loadJetBrains} from '@remotion/google-fonts/JetBrainsMono';

loadInter('normal', {weights: ['400', '500', '600', '700', '800'], subsets: ['latin']});
loadSpaceGrotesk('normal', {weights: ['500', '600', '700'], subsets: ['latin']});
loadJetBrains('normal', {weights: ['400', '500', '600', '700'], subsets: ['latin']});
